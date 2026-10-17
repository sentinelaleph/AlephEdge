//! Assistant (0.3.17): a chat over the desk's own data, on the user's own
//! OpenRouter key (BYOK).
//!
//! - The key lives ONLY in the OS keychain (`app::keychain`, like the
//!   Telegram bot token): no `Serialize`, a redacted `Debug`, wiped on drop,
//!   never logged, never returned to the UI, and sent to openrouter.ai only.
//! - Requests go from Rust straight to OpenRouter (`openrouter.rs`); the cost
//!   OpenRouter reports is kept with each answer.
//! - Tools are READ-ONLY plus `draft_bot` (`tools.rs`): the registry has no
//!   tool that starts or stops a bot or places or cancels an order, and an
//!   unknown tool name is refused. Editing the master prompt cannot change it.
//! - The master prompt is editable and versioned (`prompt.rs`); a fixed guard
//!   is always sent before it.
//! - The conversation is kept locally (`meta` table of edge.db) until the user
//!   deletes it.

pub mod commands;
pub mod openrouter;
pub mod prompt;
pub mod tools;

use std::fmt;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::app::keychain::{self, SecretStore};
use openrouter::{ToolUse, Usage};
use prompt::PromptState;
use tools::BotDraft;

/// Keychain user name of the OpenRouter key.
pub const KEY_USER: &str = "openrouter-api-key";
/// `meta` keys.
pub const SETTINGS_KEY: &str = "assistant.settings";
pub const CONVERSATION_KEY: &str = "assistant.conversation";
/// Messages kept on disk, and sent back to the model as context.
pub const CONVERSATION_CAP: usize = 200;
pub const CONTEXT_MESSAGES: usize = 20;
/// Longest user message accepted (characters).
pub const MAX_MESSAGE_CHARS: usize = 4_000;

/// An OpenRouter API key. No `Serialize`, a redacted `Debug`, wiped on drop.
#[derive(Clone, PartialEq)]
pub struct ApiKey(String);

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl ApiKey {
    /// `sk-or-...` as openrouter.ai/keys prints it.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        let ok = raw.starts_with("sk-or-")
            && (20..=256).contains(&raw.len())
            && raw.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if ok {
            Ok(Self(raw.to_string()))
        } else {
            Err("assistantKeyFormat".into())
        }
    }

    /// Only for the Authorization header of an openrouter.ai request.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

pub fn read_key(ks: &dyn SecretStore) -> Option<ApiKey> {
    let mut raw = ks.get(keychain::service(), KEY_USER)?;
    let k = ApiKey::parse(&raw).ok();
    raw.zeroize();
    k
}

pub fn write_key(ks: &dyn SecretStore, k: &ApiKey) -> Result<(), String> {
    ks.set(keychain::service(), KEY_USER, k.expose())
        .map_err(|_| "assistantKeychainFailed".to_string())
}

pub fn delete_key(ks: &dyn SecretStore) {
    ks.delete(keychain::service(), KEY_USER);
}

/// Non-secret settings (`meta`): the chosen model and the master prompt.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub model: Option<String>,
    pub prompt: PromptState,
}

/// One message of the local conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChatEntry {
    /// "user" | "assistant"
    pub role: String,
    pub text: String,
    pub at_ms: u64,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub tools: Vec<ToolUse>,
    #[serde(default)]
    pub drafts: Vec<BotDraft>,
    /// The request failed (stable code); no answer text.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub truncated: bool,
}

impl ChatEntry {
    pub fn user(text: &str, at_ms: u64) -> Self {
        Self { role: "user".into(), text: text.into(), at_ms, model: None, usage: None, tools: vec![], drafts: vec![], error: None, truncated: false }
    }
}

/// The prior messages the model sees: the last `CONTEXT_MESSAGES` that
/// carry text (failed requests and their questions are left out).
pub fn context(conv: &[ChatEntry]) -> Vec<serde_json::Value> {
    let mut ok: Vec<&ChatEntry> = Vec::new();
    for (i, e) in conv.iter().enumerate() {
        let failed_next = conv.get(i + 1).is_some_and(|n| n.error.is_some());
        if e.error.is_none() && !e.text.is_empty() && !(e.role == "user" && failed_next) {
            ok.push(e);
        }
    }
    let start = ok.len().saturating_sub(CONTEXT_MESSAGES);
    ok[start..]
        .iter()
        .map(|e| serde_json::json!({ "role": if e.role == "user" { "user" } else { "assistant" }, "content": e.text }))
        .collect()
}

/// Appends and keeps the newest `CONVERSATION_CAP`.
pub fn append(conv: &mut Vec<ChatEntry>, entries: impl IntoIterator<Item = ChatEntry>) {
    conv.extend(entries);
    if conv.len() > CONVERSATION_CAP {
        let drop = conv.len() - CONVERSATION_CAP;
        conv.drain(..drop);
    }
}

/// One request at a time.
pub struct AssistantManager {
    pub(crate) busy: std::sync::atomic::AtomicBool,
    pub(crate) models: std::sync::Mutex<Option<(u64, Vec<openrouter::ModelInfo>)>>,
}

impl AssistantManager {
    pub fn new() -> Self {
        Self { busy: std::sync::atomic::AtomicBool::new(false), models: std::sync::Mutex::new(None) }
    }
}

impl Default for AssistantManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::keychain::memory::MemoryKeychain;

    fn fake_key() -> String {
        format!("sk-or-v1-{}", "a1".repeat(32))
    }

    #[test]
    fn the_key_never_shows_in_debug_output() {
        let k = ApiKey::parse(&fake_key()).unwrap();
        assert_eq!(format!("{k:?}"), "ApiKey(<redacted>)");
        assert!(!format!("{:?}", Some(k)).contains("sk-or"));
    }

    #[test]
    fn the_key_format_is_checked_before_it_is_stored() {
        for bad in ["", "sk-or-", "sk-ant-1234567890abcdefghij", "sk-or-v1-abc def ghi jkl mno", "sk-or-v1-\"; drop"] {
            assert_eq!(ApiKey::parse(bad).unwrap_err(), "assistantKeyFormat", "{bad}");
        }
        assert!(ApiKey::parse(&format!("  {}\n", fake_key())).is_ok(), "pasted whitespace is trimmed");
    }

    #[test]
    fn the_key_round_trips_through_a_test_keychain_only() {
        let ks = MemoryKeychain::default();
        assert!(read_key(&ks).is_none());
        write_key(&ks, &ApiKey::parse(&fake_key()).unwrap()).unwrap();
        assert_eq!(ks.get(keychain::service(), KEY_USER), Some(fake_key()));
        assert_eq!(read_key(&ks).map(|k| k.expose().to_string()), Some(fake_key()));
        delete_key(&ks);
        assert!(read_key(&ks).is_none());
    }

    #[test]
    fn settings_never_carry_the_key() {
        let s = Settings { model: Some("x/y".into()), ..Default::default() };
        let body = serde_json::to_string(&s).unwrap();
        assert!(!body.contains("sk-or"));
        assert_eq!(serde_json::from_str::<Settings>(&body).unwrap(), s);
        assert_eq!(serde_json::from_str::<Settings>("{}").unwrap(), Settings::default(), "old rows load");
    }

    #[test]
    fn the_context_skips_failed_requests_and_is_bounded() {
        let mut conv = vec![ChatEntry::user("q1", 1)];
        let mut fail = ChatEntry::user("", 2);
        fail.role = "assistant".into();
        fail.error = Some("orNoCredits".into());
        conv.push(fail);
        conv.push(ChatEntry::user("q2", 3));
        let mut a = ChatEntry::user("a2", 4);
        a.role = "assistant".into();
        conv.push(a);
        let ctx = context(&conv);
        assert_eq!(ctx.len(), 2);
        assert_eq!(ctx[0]["content"], "q2");
        assert_eq!(ctx[1]["role"], "assistant");
        let mut big = Vec::new();
        append(&mut big, (0..250).map(|i| ChatEntry::user(&format!("m{i}"), i)));
        assert_eq!(big.len(), CONVERSATION_CAP);
        assert_eq!(big[0].text, "m50");
        assert_eq!(context(&big).len(), CONTEXT_MESSAGES);
    }
}
