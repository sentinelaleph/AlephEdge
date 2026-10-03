//! Where this desk talks to, and who it is.
//!
//! Every value here used to be a string literal somewhere in the UI. That is
//! survivable in a private tree and not survivable in a public one: a reader
//! who clones the repo would be pointed at one particular deployment, and
//! changing it would mean editing source rather than setting a variable.
//!
//! Three rules:
//!
//! 1. **One host, one place.** The relay is DERIVED from the API base rather
//!    than configured twice. Two knobs for one deployment is how a desk ends up
//!    authenticating against one host and streaming from another.
//! 2. **The desk id is per-device and generated, never a constant.** A literal
//!    `desk-1` in the source would give every user on earth the same desk
//!    identity, which for a pairing system is not a cosmetic problem.
//! 3. **The frontend carries no host at all.** It asks for this struct. A UI
//!    that hardcodes an endpoint will always drift from the backend that owns
//!    the real one.
//!
//! This is also where any OTHER environment knob is read (see
//! `vault_idle_minutes`), for the same reason: one file that answers "what can
//! be configured, and to what", rather than a `std::env::var` in whichever
//! module happened to need it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The deployment this build talks to when nothing overrides it.
///
/// Overridable with `ALEPH_EDGE_API_BASE`; the relay follows it unless
/// `ALEPH_EDGE_RELAY_URL` is set explicitly.
pub const DEFAULT_API_BASE: &str = "https://ribqa.com";

/// Binance USD-M futures REST base for signed account/order calls.
///
/// Overridable with `ALEPH_EDGE_BINANCE_FUTURES_BASE` so a build can be pointed
/// at the Futures TESTNET (`https://testnet.binancefuture.com`) for a dry run
/// before real money. This knob only moves WHERE orders go; it cannot enable
/// live trading — that stays the compile-time `LIVE_TRADING_ENABLED` switch.
pub const DEFAULT_BINANCE_FUTURES_BASE: &str = "https://fapi.binance.com";

/// Minutes of vault inactivity after which the decrypted exchange keys are
/// dropped from memory (`vault::idle`).
///
/// Overridable with `ALEPH_EDGE_VAULT_IDLE_MINUTES`. Thirty minutes is the
/// default because this desk is designed to sit running unattended for days:
/// before auto-lock existed, one password entry left the keys decrypted in
/// process memory until the process died, which for an unattended machine is
/// "forever".
pub const DEFAULT_VAULT_IDLE_MINUTES: u64 = 30;

/// One minute. Not zero: a zero budget would expire the session between the
/// unlock and the first credential read, which does not harden anything — it
/// makes the desk unusable, and an unusable safety feature is one the user
/// turns off.
pub const VAULT_IDLE_MINUTES_MIN: u64 = 1;

/// 24 hours. A ceiling exists so a typo ("3000") cannot quietly mean "never
/// lock" — the value that looks like a hardened configuration would be the
/// weakest one in the file.
pub const VAULT_IDLE_MINUTES_MAX: u64 = 1_440;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Endpoints {
    pub api_base: String,
    pub relay_url: String,
    /// Stable identity for THIS installation. Generated once, then persisted.
    pub desk_id: String,
    /// Where live futures orders go. Shown so a testnet build can never be
    /// mistaken for a production one (and vice versa).
    pub binance_futures_base: String,
    /// `binance_futures_base` is Binance production (not the testnet).
    #[serde(default)]
    pub binance_is_production: bool,
}

pub fn api_base() -> String {
    secure_or_default(
        base_or_default(std::env::var("ALEPH_EDGE_API_BASE").ok(), DEFAULT_API_BASE),
        DEFAULT_API_BASE,
    )
}

/// The API base carries the account password (sign-in), every access and
/// refresh token, and the stream ticket. An override like
/// `http://staging.example.com` used to be taken as-is and sent all of that in
/// clear text. Only an encrypted scheme (`https`/`wss`) is accepted, plus
/// plain `http`/`ws` to this machine itself for local development; anything
/// else falls back to the default, which is the safe end.
fn secure_or_default(base: String, default: &str) -> String {
    if is_encrypted_or_loopback(&base) {
        base
    } else {
        default.to_string()
    }
}

fn is_encrypted_or_loopback(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("wss://") {
        return true;
    }
    let Some(rest) = lower
        .strip_prefix("http://")
        .or_else(|| lower.strip_prefix("ws://"))
    else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Userinfo would let `http://localhost@evil.example` pass a prefix check.
    if authority.contains('@') {
        return false;
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host.ends_with(".localhost")
}

pub fn binance_futures_base() -> String {
    futures_base_or_default(std::env::var("ALEPH_EDGE_BINANCE_FUTURES_BASE").ok())
}

/// Binance's USDⓈ-M testnet, the only override the futures base accepts.
pub const BINANCE_FUTURES_TESTNET_BASE: &str = "https://testnet.binancefuture.com";

/// Signed requests carry the API key and a valid signature: an override to
/// any other host (or to plain http) would hand both to it, and a signed
/// order can be replayed against Binance within its recvWindow. So the base
/// is production or the testnet, and anything else falls back to production.
fn futures_base_or_default(raw: Option<String>) -> String {
    let base = base_or_default(raw, DEFAULT_BINANCE_FUTURES_BASE);
    if base.eq_ignore_ascii_case(BINANCE_FUTURES_TESTNET_BASE) {
        BINANCE_FUTURES_TESTNET_BASE.to_string()
    } else {
        DEFAULT_BINANCE_FUTURES_BASE.to_string()
    }
}

/// False when `ALEPH_EDGE_BINANCE_FUTURES_BASE` points somewhere else (the
/// testnet). The UI reads this flag rather than comparing hosts itself.
pub fn binance_is_production() -> bool {
    binance_futures_base() == DEFAULT_BINANCE_FUTURES_BASE
}

/// Minutes of vault idleness before auto-lock, from the environment.
pub fn vault_idle_minutes() -> u64 {
    idle_minutes_or_default(std::env::var("ALEPH_EDGE_VAULT_IDLE_MINUTES").ok())
}

/// Same shape as `base_or_default`: one pure function per knob so the rule is
/// tested rather than re-implemented at the call site.
///
/// Anything unparseable falls back to the default rather than failing the
/// launch. A desk that refuses to start because of a stray character in an
/// optional variable is worse than a desk on the documented 30 minutes, and
/// there is no value here an attacker gains by supplying garbage: the fallback
/// is the safe end.
fn idle_minutes_or_default(raw: Option<String>) -> u64 {
    raw.and_then(|s| s.trim().parse::<u64>().ok())
        .map(|m| m.clamp(VAULT_IDLE_MINUTES_MIN, VAULT_IDLE_MINUTES_MAX))
        .unwrap_or(DEFAULT_VAULT_IDLE_MINUTES)
}

/// One normalisation for every base override: trimmed, no trailing slash,
/// empty means unset. Pure so the rule is tested, not re-implemented.
fn base_or_default(raw: Option<String>, default: &str) -> String {
    raw.map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Turns the API base into a websocket origin: `https` → `wss`, `http` → `ws`.
///
/// Derived rather than configured, so a deployment cannot end up half-moved.
/// An explicit `ALEPH_EDGE_RELAY_URL` still wins, because a relay does not have
/// to live on the API host.
pub fn relay_url() -> String {
    if let Some(explicit) = std::env::var("ALEPH_EDGE_RELAY_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        // The relay handshake carries the access token too.
        .filter(|s| is_encrypted_or_loopback(s))
    {
        return explicit;
    }
    ws_origin(&api_base())
}

/// The derivation itself, pure so it can be tested without touching the
/// environment — and so there is exactly ONE implementation of it. A test that
/// re-implements the rule it is checking passes while the shipped rule is
/// broken; this repo has paid for that lesson three times.
fn ws_origin(base: &str) -> String {
    if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        // Already a websocket scheme, or something we do not recognise: hand it
        // back untouched rather than guessing. A silently rewritten endpoint is
        // harder to debug than one that plainly does not connect.
        base.to_string()
    }
}

/// Reads the persisted desk id, generating one on first run.
///
/// Not random-per-launch: the phone pairs with an identity and has to find the
/// same desk tomorrow.
pub fn desk_id(app_data_dir: &Path) -> String {
    let path = desk_id_path(app_data_dir);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    let fresh = generate_desk_id();
    // A write failure is not fatal — the desk still works this session, it just
    // gets a new identity next launch and the phone must pair again. Better
    // than refusing to start.
    //
    // Owner-only and atomic for the same reason as the vault: this is the
    // identity the phone pairs against, so a world-readable copy hands a local
    // attacker the name to impersonate on the relay, and a half-written file
    // silently re-identifies the desk and unpairs the phone.
    let _ = crate::vault::secure_file::write_private_atomic(&path, fresh.as_bytes());
    fresh
}

fn desk_id_path(dir: &Path) -> PathBuf {
    dir.join("desk-id")
}

fn generate_desk_id() -> String {
    format!("desk-{}", uuid::Uuid::new_v4().simple())
}

pub fn endpoints(app_data_dir: &Path) -> Endpoints {
    Endpoints {
        api_base: api_base(),
        relay_url: relay_url(),
        desk_id: desk_id(app_data_dir),
        binance_futures_base: binance_futures_base(),
        binance_is_production: binance_is_production(),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_futures_base_is_production_or_the_testnet_only() {
        assert_eq!(futures_base_or_default(None), DEFAULT_BINANCE_FUTURES_BASE);
        assert_eq!(
            futures_base_or_default(Some("https://testnet.binancefuture.com/".into())),
            BINANCE_FUTURES_TESTNET_BASE
        );
        for other in ["http://testnet.binancefuture.com", "https://evil.example", "http://127.0.0.1:9000", "https://fapi.binance.com.evil.example"] {
            assert_eq!(futures_base_or_default(Some(other.into())), DEFAULT_BINANCE_FUTURES_BASE, "{other}");
        }
    }

    use super::*;

    #[test]
    fn relay_is_derived_from_the_api_base() {
        // One deployment, one knob. Configuring both invites a desk that
        // authenticates against one host and streams from another.
        assert_eq!(ws_origin("https://example.test"), "wss://example.test");
        assert_eq!(ws_origin("http://localhost:8080"), "ws://localhost:8080");
    }

    #[test]
    fn a_plain_http_api_base_is_refused_unless_it_is_this_machine() {
        // The password and every token travel to this host.
        let d = DEFAULT_API_BASE;
        assert_eq!(secure_or_default("http://staging.example.com".into(), d), d);
        assert_eq!(secure_or_default("ftp://x".into(), d), d);
        assert_eq!(secure_or_default("http://localhost@evil.example".into(), d), d);
        assert_eq!(secure_or_default("http://127.0.0.1.evil.example".into(), d), d);
        for ok in [
            "https://staging.example.com",
            "http://localhost:8080",
            "http://127.0.0.1:8080",
            "http://[::1]:8080",
            "HTTPS://ribqa.com",
        ] {
            assert_eq!(secure_or_default(ok.into(), d), ok);
        }
        assert!(is_encrypted_or_loopback("wss://ribqa.com"));
        assert!(!is_encrypted_or_loopback("ws://relay.example.com"));
    }

    #[test]
    fn an_unrecognised_scheme_is_handed_back_untouched() {
        // A silently rewritten endpoint is harder to debug than one that
        // plainly refuses to connect.
        assert_eq!(ws_origin("wss://already.ws"), "wss://already.ws");
    }

    #[test]
    fn base_overrides_are_normalised_and_default_to_production() {
        let d = DEFAULT_BINANCE_FUTURES_BASE;
        assert_eq!(base_or_default(None, d), "https://fapi.binance.com");
        assert_eq!(base_or_default(Some("  ".into()), d), d);
        assert_eq!(
            base_or_default(Some(" https://testnet.binancefuture.com/ ".into()), d),
            "https://testnet.binancefuture.com"
        );
    }

    #[test]
    fn the_idle_budget_defaults_to_thirty_minutes_and_is_clamped() {
        // The default is the documented one, garbage falls back to it, and both
        // ends are clamped: a 0 would lock the vault before the first
        // credential read, and an unbounded value would let a typo mean
        // "never lock" while looking like configuration.
        assert_eq!(idle_minutes_or_default(None), 30);
        assert_eq!(idle_minutes_or_default(Some("  ".into())), 30);
        assert_eq!(idle_minutes_or_default(Some("soon".into())), 30);
        assert_eq!(idle_minutes_or_default(Some(" 5 ".into())), 5);
        assert_eq!(
            idle_minutes_or_default(Some("0".into())),
            VAULT_IDLE_MINUTES_MIN
        );
        assert_eq!(
            idle_minutes_or_default(Some("99999".into())),
            VAULT_IDLE_MINUTES_MAX
        );
    }

    #[test]
    fn desk_ids_are_unique_per_installation() {
        // A constant here would hand every user on earth the same desk
        // identity. For a pairing system that is not cosmetic.
        assert_ne!(generate_desk_id(), generate_desk_id());
        assert!(generate_desk_id().starts_with("desk-"));
    }

    #[test]
    fn a_generated_desk_id_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("aleph-desk-{}", uuid::Uuid::new_v4()));
        let first = desk_id(&dir);
        let second = desk_id(&dir);
        assert_eq!(first, second, "the phone must find the same desk tomorrow");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
