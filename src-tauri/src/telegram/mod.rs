//! Optional Telegram notifications and two opt-in commands.
//!
//! The in-app alerts are worked out in the webview, which does not run while
//! the window sits in the tray; this module reads the same real sources from
//! Rust and sends them to ONE linked Telegram chat.
//!
//! - The bot token lives only in the OS keychain (`app::keychain`), never in
//!   a file, a log or the webview. The UI learns `tokenSet` and the bot's
//!   @username.
//! - A chat is linked by a one-time code the user sends as `/start <code>`
//!   from a private chat. Every other chat is ignored, and only the linked
//!   chat may run a command.
//! - `/status` and `/stop` are off by default. `/stop` stops NEW entries on
//!   every bot (the phone link's "Stop"); it closes nothing.
//!
//! Two modes (`Mode`): `Own`, everything above, sent straight from this
//! computer; `Official`, the same notifications through Sentinel's own bot
//! (@SentinelAleph_bot, `official.rs`) with the signed-in session, no token
//! and no commands. New installs start on `Official`; settings saved before
//! modes existed load as `Own` when a token is in the keychain.

pub mod api;
pub mod commands;
pub mod notify;
pub mod official;
mod tasks;

use std::fmt;
use std::sync::Mutex;

use rand::Rng;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::app::keychain::{self, SecretStore};
use api::Message;
use notify::{Categories, Cursors, Watch};

pub use tasks::spawn;

/// Keychain user name of the bot token.
pub const TOKEN_USER: &str = "telegram-bot-token";
/// `meta` key of the non-secret settings.
pub const SETTINGS_KEY: &str = "telegram.settings";
/// How long a link code is accepted.
pub const LINK_TTL_MS: u64 = 10 * 60 * 1000;
/// A command older than this when read is not run (it may have waited on
/// Telegram's side while the app was closed or commands were off).
pub const COMMAND_MAX_AGE_MS: u64 = 120_000;

/// A bot token. No `Serialize`, a redacted `Debug`, wiped on drop.
#[derive(Clone, PartialEq)]
pub struct BotToken(String);

impl fmt::Debug for BotToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BotToken(<redacted>)")
    }
}

impl Drop for BotToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl BotToken {
    /// `<bot id>:<secret>` as @BotFather prints it.
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        let (id, secret) = raw.split_once(':').ok_or("telegramTokenFormat")?;
        let id_ok = (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit());
        let secret_ok = (30..=64).contains(&secret.len())
            && secret.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
        if id_ok && secret_ok {
            Ok(Self(raw.to_string()))
        } else {
            Err("telegramTokenFormat".into())
        }
    }

    /// Only for building a request URL.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

pub fn read_token(ks: &dyn SecretStore) -> Option<BotToken> {
    let mut raw = ks.get(keychain::service(), TOKEN_USER)?;
    let t = BotToken::parse(&raw).ok();
    raw.zeroize();
    t
}

pub fn write_token(ks: &dyn SecretStore, t: &BotToken) -> Result<(), String> {
    ks.set(keychain::service(), TOKEN_USER, t.expose())
        .map_err(|_| "telegramKeychainFailed".to_string())
}

pub fn delete_token(ks: &dyn SecretStore) {
    ks.delete(keychain::service(), TOKEN_USER);
}

/// Which bot sends: Sentinel's official one, or the user's own.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Official,
    Own,
}

/// Everything but the token, kept in the store's `meta` table.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// `None` only in rows saved before modes existed (see `migrate`).
    pub mode: Option<Mode>,
    pub enabled: bool,
    /// Own bot only: the official bot relays no commands.
    pub commands_enabled: bool,
    pub chat_id: Option<i64>,
    pub chat_label: Option<String>,
    pub bot_username: Option<String>,
    pub categories: Categories,
    pub cursors: Cursors,
    /// Next `getUpdates` offset.
    pub update_offset: i64,
    /// The server said a chat is linked to the official bot (last read).
    pub official_linked: bool,
    pub official_chat_label: Option<String>,
    pub official_linked_at: Option<u64>,
}

impl Settings {
    /// Gives a row without a mode one: `Own` for a user who already saved a
    /// token (nothing changes for them), `Official` for everyone else.
    /// `true` when it changed the row.
    pub fn migrate(&mut self, token_present: bool) -> bool {
        if self.mode.is_some() {
            return false;
        }
        self.mode = Some(if token_present { Mode::Own } else { Mode::Official });
        true
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PendingLink {
    pub code: String,
    pub expires_at_ms: u64,
}

/// An unambiguous 8-character code (no 0/O, 1/I/L).
pub fn new_link_code() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..8).map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char).collect()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd {
    Status,
    Stop,
    Help,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Link { chat_id: i64, label: Option<String> },
    Command { chat_id: i64, cmd: Cmd },
    CommandsOff { chat_id: i64 },
    Ignore,
}

/// The code in `/start <code>` (or `/start@bot <code>`), if that is the
/// whole message.
fn start_code(text: &str) -> Option<&str> {
    let mut words = text.split_whitespace();
    let head = words.next()?;
    let code = words.next()?;
    if words.next().is_some() {
        return None;
    }
    (head == "/start" || head.starts_with("/start@")).then_some(code)
}

/// What to do with one incoming message. The only gate for commands: the
/// linked chat id. A link needs the live code from a private chat.
pub fn route(msg: &Message, linked: Option<i64>, pending: Option<&PendingLink>, commands_on: bool, now_ms: u64) -> Route {
    let Some(text) = msg.text.as_deref().map(str::trim) else {
        return Route::Ignore;
    };
    if let Some(p) = pending {
        if p.expires_at_ms > now_ms && msg.chat.kind == "private" && start_code(text) == Some(p.code.as_str()) {
            return Route::Link { chat_id: msg.chat.id, label: msg.chat.label() };
        }
    }
    if linked != Some(msg.chat.id) || !text.starts_with('/') {
        return Route::Ignore;
    }
    let sent_ms = u64::try_from(msg.date).unwrap_or(0).saturating_mul(1000);
    if sent_ms + COMMAND_MAX_AGE_MS < now_ms {
        return Route::Ignore;
    }
    if !commands_on {
        return Route::CommandsOff { chat_id: msg.chat.id };
    }
    let word = text.split_whitespace().next().unwrap_or("");
    let word = word.split('@').next().unwrap_or("").to_ascii_lowercase();
    let cmd = match word.as_str() {
        "/status" => Cmd::Status,
        "/stop" => Cmd::Stop,
        _ => Cmd::Help,
    };
    Route::Command { chat_id: msg.chat.id, cmd }
}

/// What the UI sees. No token, no chat id.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStatus {
    pub mode: Mode,
    pub official: OfficialView,
    pub token_set: bool,
    pub bot_username: Option<String>,
    pub linked: bool,
    pub chat_label: Option<String>,
    pub linking: Option<LinkingView>,
    pub prefs: Prefs,
    pub last_error: Option<String>,
    pub last_sent_at: Option<u64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LinkingView {
    pub code: String,
    pub expires_at_ms: u64,
}

/// The official bot as the panel shows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OfficialView {
    /// The bot's @username, without the @.
    pub bot: String,
    /// From the last server read; `None` before one.
    pub available: Option<bool>,
    /// `None` until checked.
    pub signed_in: Option<bool>,
    pub linked: bool,
    pub chat_label: Option<String>,
    pub linked_at: Option<u64>,
    /// A code waiting for `/start <code>` to the official bot.
    pub linking: Option<OfficialLinkingView>,
    /// The chat blocked the bot (409 tgBlocked): nothing is sent.
    pub blocked: bool,
    /// Signed out: sending is paused.
    pub paused: bool,
    /// The last send met 503 tgUnavailable.
    pub unavailable: bool,
    /// Lines waiting in the outbox.
    pub queued: usize,
    /// Why the last server read failed (`errors.*` code).
    pub check_error: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OfficialLinkingView {
    pub code: String,
    pub expires_at_ms: u64,
    pub bot: String,
}

/// Where a message goes.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    Own(BotToken, i64),
    Official,
}

/// The user's choices, in and out.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    pub enabled: bool,
    pub commands_enabled: bool,
    pub categories: Categories,
}

#[derive(Default)]
pub(crate) struct Inner {
    pub loaded: bool,
    pub token: Option<BotToken>,
    pub settings: Settings,
    pub link: Option<PendingLink>,
    pub watch: Watch,
    /// (code, came from polling): a poll success clears only a poll error.
    pub last_error: Option<(String, bool)>,
    pub last_sent_at: Option<u64>,
    /// Bumped by every reset / link / removal: a pass computed before it is
    /// not committed after it.
    pub generation: u64,
    /// Official bot: the live link code, if any.
    pub official_link: Option<official::LinkReply>,
    pub official_bot: Option<String>,
    pub official_available: Option<bool>,
    pub official_signed_in: Option<bool>,
    pub official_blocked: bool,
    pub official_paused: bool,
    pub official_unavailable: bool,
    pub official_queued: usize,
    pub official_check_error: Option<String>,
}

impl Inner {
    /// The chosen mode; a row not migrated yet reads as `migrate` would.
    pub fn mode(&self) -> Mode {
        self.settings.mode.unwrap_or(if self.token.is_some() { Mode::Own } else { Mode::Official })
    }

    pub fn view(&self, now_ms: u64) -> TelegramStatus {
        let s = &self.settings;
        let bot = self.official_bot.clone().unwrap_or_else(|| official::BOT.to_string());
        TelegramStatus {
            mode: self.mode(),
            official: OfficialView {
                bot: bot.clone(),
                available: self.official_available,
                signed_in: self.official_signed_in,
                linked: s.official_linked,
                chat_label: s.official_chat_label.clone(),
                linked_at: s.official_linked_at,
                linking: self
                    .official_link
                    .as_ref()
                    .filter(|l| l.expires_at_ms > now_ms && !s.official_linked)
                    .map(|l| OfficialLinkingView { code: l.code.clone(), expires_at_ms: l.expires_at_ms, bot: l.bot.clone() }),
                blocked: self.official_blocked,
                paused: self.official_paused,
                unavailable: self.official_unavailable,
                queued: self.official_queued,
                check_error: self.official_check_error.clone(),
            },
            token_set: self.token.is_some(),
            bot_username: s.bot_username.clone(),
            linked: s.chat_id.is_some(),
            chat_label: s.chat_label.clone(),
            linking: self
                .link
                .as_ref()
                .filter(|l| l.expires_at_ms > now_ms)
                .map(|l| LinkingView { code: l.code.clone(), expires_at_ms: l.expires_at_ms }),
            prefs: Prefs { enabled: s.enabled, commands_enabled: s.commands_enabled, categories: s.categories },
            last_error: self.last_error.as_ref().map(|(e, _)| e.clone()),
            last_sent_at: self.last_sent_at,
        }
    }

    /// Notifications run with the switch on and, by mode: a token and a
    /// linked chat (own); a chat linked to the official bot that has not
    /// blocked it (official).
    pub fn active(&self) -> Option<Target> {
        if !self.settings.enabled {
            return None;
        }
        match self.mode() {
            Mode::Own => match (&self.token, self.settings.chat_id) {
                (Some(t), Some(chat)) => Some(Target::Own(t.clone(), chat)),
                _ => None,
            },
            Mode::Official => (self.settings.official_linked && !self.official_blocked).then_some(Target::Official),
        }
    }

    /// A chat is linked in the current mode.
    pub fn linked(&self) -> bool {
        match self.mode() {
            Mode::Own => self.settings.chat_id.is_some(),
            Mode::Official => self.settings.official_linked,
        }
    }

    /// `getUpdates` runs (own bot only) while a code waits, or while
    /// commands are on.
    pub fn should_poll(&self, now_ms: u64) -> bool {
        self.mode() == Mode::Own
            && self.token.is_some()
            && (self.link.as_ref().is_some_and(|l| l.expires_at_ms > now_ms)
                || (self.settings.chat_id.is_some() && self.settings.commands_enabled))
    }

    /// The server linked a chat to the official bot (or still has it).
    /// `true` when this is a new link: the caller starts from "now".
    pub fn official_linked(&mut self, label: Option<String>, at: Option<u64>) -> bool {
        let fresh = !self.settings.official_linked;
        self.settings.official_linked = true;
        self.settings.official_chat_label = label;
        self.settings.official_linked_at = at;
        self.official_link = None;
        if fresh {
            self.official_blocked = false;
            if self.mode() == Mode::Official {
                self.settings.enabled = true;
            }
            self.generation += 1;
        }
        fresh
    }

    /// Forgets the official chat (unlinked here, on the server, or 409).
    pub fn clear_official(&mut self) {
        self.settings.official_linked = false;
        self.settings.official_chat_label = None;
        self.settings.official_linked_at = None;
        self.official_link = None;
        self.official_blocked = false;
        if self.mode() == Mode::Official {
            self.settings.enabled = false;
        }
        self.generation += 1;
    }

    /// Forgets the chat and the bot (a new token may be another bot).
    pub fn clear_link(&mut self) {
        self.settings.chat_id = None;
        self.settings.chat_label = None;
        self.settings.enabled = false;
        self.settings.commands_enabled = false;
        self.settings.update_offset = 0;
        self.link = None;
        self.watch = Watch::default();
        self.generation += 1;
    }
}

/// One line to the linked chat, under the Paper header, when notifications
/// and `category` are on; nothing otherwise. Used by the TradingView link
/// for an alert that was not executed.
pub(crate) async fn send_line(app: &tauri::AppHandle, category: notify::Category, body: &str) {
    send_line_for(app, category, body, false).await;
}

/// `send_line` under the Paper or LIVE header (`live`): the portfolio risk
/// view says which book crossed a threshold.
pub(crate) async fn send_line_for(app: &tauri::AppHandle, category: notify::Category, body: &str, live: bool) {
    use tauri::Manager;
    let target = {
        let mgr = app.state::<TelegramManager>();
        let g = mgr.lock();
        g.active().filter(|_| g.settings.categories.allows(&[category]))
    };
    let text = format!("{}\n{body}", notify::header(live));
    match target {
        Some(Target::Own(token, chat)) => {
            let _ = tasks::send(app, &token, chat, &text).await;
        }
        Some(Target::Official) => tasks::queue_official(app, vec![text]).await,
        None => {}
    }
}

pub struct TelegramManager {
    pub(crate) inner: Mutex<Inner>,
    /// Time of the last send (ms): at most one message a second.
    pub(crate) send_gate: tokio::sync::Mutex<u64>,
    pub(crate) api: api::Api,
    pub(crate) official: official::OfficialApi,
    /// Official-mode lines waiting for the relay. Held across a drain.
    pub(crate) outbox: tokio::sync::Mutex<official::Outbox>,
}

impl TelegramManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            send_gate: tokio::sync::Mutex::new(0),
            api: api::Api::new(api::api_base()),
            official: official::OfficialApi::new(),
            outbox: tokio::sync::Mutex::new(official::Outbox::default()),
        }
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl Default for TelegramManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::keychain::memory::MemoryKeychain;
    use api::Chat;

    /// Built at run time so no token-shaped literal sits in the source.
    fn fake() -> String {
        format!("{}:{}", 123_456_789, SECRET_PART.repeat(3))
    }
    const SECRET_PART: &str = "fakeTOKENpart_-";

    fn msg(chat: i64, kind: &str, text: &str, date_s: i64) -> Message {
        Message {
            chat: Chat { id: chat, kind: kind.into(), username: Some("ann".into()), first_name: None },
            date: date_s,
            text: Some(text.into()),
        }
    }

    #[test]
    fn the_token_never_shows_in_debug_output() {
        let t = BotToken::parse(&fake()).unwrap();
        assert_eq!(format!("{t:?}"), "BotToken(<redacted>)");
        let inner = Inner { token: Some(t.clone()), ..Default::default() };
        let view = format!("{:?}", inner.view(0));
        assert!(!view.contains(SECRET_PART), "{view}");
        let json = serde_json::to_string(&inner.view(0)).unwrap();
        assert!(!json.contains(SECRET_PART) && json.contains("\"tokenSet\":true"), "{json}");
        let settings = serde_json::to_string(&inner.settings).unwrap();
        assert!(!settings.contains(SECRET_PART));
    }

    #[test]
    fn token_format_is_checked_before_any_call() {
        assert!(BotToken::parse(&format!("  {}\n", fake())).is_ok());
        let long = SECRET_PART.repeat(3);
        let bad = [String::new(), "123456".into(), format!("abc:{long}"), "123:short".into(), format!("123:fake {long}")];
        for bad in bad {
            assert_eq!(BotToken::parse(&bad).unwrap_err(), "telegramTokenFormat", "{bad}");
        }
    }

    #[test]
    fn the_token_round_trips_through_a_test_keychain_only() {
        let ks = MemoryKeychain::default();
        assert!(read_token(&ks).is_none());
        write_token(&ks, &BotToken::parse(&fake()).unwrap()).unwrap();
        assert_eq!(ks.get(keychain::service(), TOKEN_USER), Some(fake()));
        assert_eq!(read_token(&ks), Some(BotToken::parse(&fake()).unwrap()));
        delete_token(&ks);
        assert!(read_token(&ks).is_none());
    }

    #[test]
    fn link_codes_are_eight_unambiguous_characters() {
        let a = new_link_code();
        assert_eq!(a.len(), 8);
        assert!(a.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
        assert!(!a.contains(['0', 'O', '1', 'I', 'L']));
    }

    #[test]
    fn only_the_live_code_from_a_private_chat_links() {
        let now = 1_700_000_000_000;
        let p = PendingLink { code: "K7M2Q9XR".into(), expires_at_ms: now + 1000 };
        assert_eq!(
            route(&msg(42, "private", "/start K7M2Q9XR", 0), None, Some(&p), false, now),
            Route::Link { chat_id: 42, label: Some("@ann".into()) }
        );
        assert_eq!(
            route(&msg(42, "private", "/start@aleph_bot K7M2Q9XR", 0), None, Some(&p), false, now),
            Route::Link { chat_id: 42, label: Some("@ann".into()) }
        );
        for wrong in ["/start", "/start k7m2q9xr", "/start K7M2Q9XRX", "K7M2Q9XR", "/start K7M2Q9XR extra"] {
            assert_eq!(route(&msg(42, "private", wrong, 0), None, Some(&p), false, now), Route::Ignore, "{wrong}");
        }
        assert_eq!(route(&msg(-5, "group", "/start K7M2Q9XR", 0), None, Some(&p), false, now), Route::Ignore, "groups never link");
        let expired = PendingLink { expires_at_ms: now, ..p };
        assert_eq!(route(&msg(42, "private", "/start K7M2Q9XR", 0), None, Some(&expired), false, now), Route::Ignore);
    }

    #[test]
    fn commands_run_only_from_the_linked_chat() {
        let now = 1_700_000_000_000;
        let fresh = (now / 1000) as i64;
        assert_eq!(
            route(&msg(42, "private", "/status", fresh), Some(42), None, true, now),
            Route::Command { chat_id: 42, cmd: Cmd::Status }
        );
        assert_eq!(
            route(&msg(42, "private", "/stop@aleph_bot", fresh), Some(42), None, true, now),
            Route::Command { chat_id: 42, cmd: Cmd::Stop }
        );
        assert_eq!(
            route(&msg(42, "private", "/close", fresh), Some(42), None, true, now),
            Route::Command { chat_id: 42, cmd: Cmd::Help }
        );
        // Another chat: ignored, whatever it sends.
        assert_eq!(route(&msg(43, "private", "/stop", fresh), Some(42), None, true, now), Route::Ignore);
        assert_eq!(route(&msg(43, "private", "/stop", fresh), None, None, true, now), Route::Ignore);
        // Plain text is not a command.
        assert_eq!(route(&msg(42, "private", "stop", fresh), Some(42), None, true, now), Route::Ignore);
        // Commands off: the linked chat is told so, nothing runs.
        assert_eq!(route(&msg(42, "private", "/stop", fresh), Some(42), None, false, now), Route::CommandsOff { chat_id: 42 });
        // A command that waited on Telegram's side is not run late.
        assert_eq!(route(&msg(42, "private", "/stop", fresh - 600), Some(42), None, true, now), Route::Ignore);
    }

    #[test]
    fn polling_and_sending_need_their_preconditions() {
        let mut i = Inner::default();
        assert!(i.active().is_none() && !i.should_poll(0));
        i.token = Some(BotToken::parse(&fake()).unwrap());
        assert!(!i.should_poll(0));
        i.link = Some(PendingLink { code: "X".into(), expires_at_ms: 10 });
        assert!(i.should_poll(0) && !i.should_poll(10));
        i.link = None;
        i.settings.chat_id = Some(42);
        assert!(i.active().is_none(), "linked but switched off");
        i.settings.enabled = true;
        assert!(matches!(i.active(), Some(Target::Own(_, 42))));
        assert!(!i.should_poll(0), "commands off: no polling");
        i.settings.commands_enabled = true;
        assert!(i.should_poll(0));
        let g = i.generation;
        i.clear_link();
        assert!(i.settings.chat_id.is_none() && !i.settings.enabled && !i.settings.commands_enabled);
        assert_eq!(i.generation, g + 1);
    }

    #[test]
    fn settings_survive_a_round_trip_and_old_rows() {
        let s = Settings { enabled: true, chat_id: Some(42), update_offset: 9, ..Default::default() };
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        let partial: Settings = serde_json::from_str(r#"{"enabled":true}"#).unwrap();
        assert!(partial.enabled && partial.categories.deal_closed && !partial.commands_enabled);
    }

    /// A 0.3.15 row (no `mode`, no official fields).
    const OLD_ROW: &str = r#"{"enabled":true,"commandsEnabled":true,"chatId":42,"chatLabel":"@ann","botUsername":"my_bot",
        "categories":{"killSwitch":true,"botError":true,"stopLoss":false,"dealClosed":true},
        "cursors":{"tradeId":7,"cycleClosedAt":0,"skipAtMs":0,"foreignAtMs":0},"updateOffset":11}"#;

    #[test]
    fn an_old_row_with_a_token_stays_on_the_own_bot() {
        let mut s: Settings = serde_json::from_str(OLD_ROW).unwrap();
        assert_eq!(s.mode, None);
        assert!(s.migrate(true));
        assert_eq!(s.mode, Some(Mode::Own));
        assert!(s.enabled && s.commands_enabled && s.chat_id == Some(42) && s.update_offset == 11 && !s.categories.stop_loss);
        assert!(!s.migrate(false), "migrated once; a later missing token does not flip it");
        assert_eq!(s.mode, Some(Mode::Own));
        let inner = Inner { token: Some(BotToken::parse(&fake()).unwrap()), settings: s, ..Default::default() };
        assert!(matches!(inner.active(), Some(Target::Own(_, 42))));
    }

    #[test]
    fn no_token_means_the_official_bot() {
        let mut fresh = Settings::default();
        assert!(fresh.migrate(false));
        assert_eq!(fresh.mode, Some(Mode::Official));
        let mut old: Settings = serde_json::from_str(OLD_ROW).unwrap();
        old.migrate(false);
        assert_eq!(old.mode, Some(Mode::Official));
        // Before load ran: the same answer from the token alone.
        assert_eq!(Inner::default().mode(), Mode::Official);
        assert_eq!(Inner::default().view(0).mode, Mode::Official);
        let json = serde_json::to_string(&fresh).unwrap();
        assert!(json.contains(r#""mode":"official""#), "{json}");
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back, fresh);
    }

    #[test]
    fn the_official_bot_sends_only_when_linked_and_not_blocked() {
        let mut i = Inner { settings: Settings { mode: Some(Mode::Official), ..Default::default() }, ..Default::default() };
        assert!(i.active().is_none() && !i.linked());
        let g = i.generation;
        assert!(i.official_linked(Some("@ann".into()), Some(5)), "a new link");
        assert!(i.settings.enabled, "linking turns notifications on");
        assert_eq!(i.generation, g + 1);
        assert_eq!(i.active(), Some(Target::Official));
        assert!(!i.official_linked(Some("@ann".into()), Some(5)), "the same link again is not new");
        i.official_blocked = true;
        assert!(i.active().is_none(), "blocked: nothing is sent");
        i.official_blocked = false;
        // No getUpdates in official mode, even with a token and commands on.
        i.token = Some(BotToken::parse(&fake()).unwrap());
        i.settings.chat_id = Some(42);
        i.settings.commands_enabled = true;
        assert!(!i.should_poll(0));
        assert_eq!(i.active(), Some(Target::Official), "the own chat does not matter here");
        let v = i.view(0);
        assert!(v.official.linked && v.official.chat_label.as_deref() == Some("@ann") && v.official.bot == official::BOT);
        i.clear_official();
        assert!(!i.settings.enabled && !i.settings.official_linked && i.active().is_none());
        // The own chat is untouched by the official one.
        assert_eq!(i.settings.chat_id, Some(42));
    }

    #[test]
    fn a_live_official_code_shows_until_linked() {
        let mut i = Inner { settings: Settings { mode: Some(Mode::Official), ..Default::default() }, ..Default::default() };
        i.official_link = Some(official::LinkReply {
            code: "K7M2Q9XR".into(),
            expires_at_ms: 100,
            bot: "SentinelAleph_bot".into(),
            deep_link: "https://t.me/SentinelAleph_bot?start=K7M2Q9XR".into(),
        });
        assert_eq!(i.view(50).official.linking.map(|l| l.code), Some("K7M2Q9XR".into()));
        assert!(i.view(100).official.linking.is_none(), "expired");
        let json = serde_json::to_string(&i.view(50)).unwrap();
        assert!(!json.contains("t.me"), "the deep link stays in Rust: {json}");
        i.official_linked(None, None);
        assert!(i.view(50).official.linking.is_none());
    }
}
