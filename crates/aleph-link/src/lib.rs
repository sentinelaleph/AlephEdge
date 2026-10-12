//! Aleph Edge: the signed message protocol between the desktop and the phone.
//!
//! # Why a single crate
//!
//! The phone connects to the Aleph Edge running on the user's own computer and
//! sends it commands: show status, stop the bot, close everything. The path
//! runs through a relay we operate, because a home computer sits behind NAT.
//!
//! **The relay carries messages; it cannot forge them.** This crate is what
//! guarantees that: every command is signed with the shared key set up during
//! pairing, and the relay does not have that key.
//!
//! Two implementations of this logic (Rust on the desktop, TypeScript on the
//! phone) would drift apart over time, and here the drift would sit on a
//! security boundary. So the protocol lives in one place and both ends link it.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Protocol version. A mismatched version is rejected, never silently
/// interpreted.
///
/// 1 -> 2: the envelope gained `epoch` (see `ReplayGuard`). Adding a field
/// makes an older peer's envelope undecodable, so the version was bumped as
/// well; silently reading the missing field as zero would let the two ends
/// believe they speak the same protocol when they do not.
pub const PROTOCOL_VERSION: u8 = 2;

/// Maximum clock skew (ms) at which an envelope is still accepted.
///
/// Phone and desktop clocks disagree; 90 seconds allows a reasonable skew
/// while stopping a recorded envelope from being replayed days later. It is
/// not enough on its own: the real replay protection is `seq`.
pub const MAX_CLOCK_SKEW_MS: u64 = 90_000;

/// Pairing key: the secret the phone and the desktop share.
///
/// It travels by QR: the desktop shows it on screen, the phone reads it with
/// the camera. The relay has no access to this key and must never have it.
#[derive(Clone)]
pub struct PairingKey([u8; 32]);

impl PairingKey {
    pub fn from_bytes(b: [u8; 32]) -> Self {
        Self(b)
    }

    /// Generates a new key (the desktop calls this when pairing begins).
    pub fn generate() -> Self {
        use rand_core::RngCore;
        let mut b = [0u8; 32];
        rand_core::OsRng.fill_bytes(&mut b);
        Self(b)
    }

    /// The text printed into the QR.
    pub fn to_pairing_string(&self) -> String {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_pairing_string(s: &str) -> Result<Self, LinkError> {
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let raw = URL_SAFE_NO_PAD
            .decode(s.trim())
            .map_err(|_| LinkError::BadPairingString)?;
        let b: [u8; 32] = raw.try_into().map_err(|_| LinkError::BadPairingString)?;
        Ok(Self(b))
    }
}

// The key must never reach a log line or an error message.
impl std::fmt::Debug for PairingKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PairingKey(<redacted>)")
    }
}

/// Commands from the phone to the desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Ask for the desk's current state.
    Status,
    /// Stop opening new positions. Open positions keep running.
    ///
    /// Kept SEPARATE from `CloseAll` and never merged with it. In an emergency
    /// control the word "kill" is too ambiguous: a user who presses "stop" and
    /// believes the open positions closed too, or believes the reverse, pays
    /// for it either way. The desktop draws the same line (the risk setting
    /// `close_on_stop`).
    StopOpening { bot: BotSelector },
    /// Close the open positions as well.
    CloseAll { bot: BotSelector },
    /// Start the bot again.
    Start { bot: BotSelector },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotSelector {
    All,
    Futures,
    Spot,
    Pump,
}

/// Messages from the desktop to the phone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "msg", rename_all = "snake_case")]
pub enum DeskMessage {
    /// Regular heartbeat. The phone watches for it to STOP.
    ///
    /// The app's most valuable notification is not "new signal" but "your desk
    /// stopped", which makes this the most important message in the protocol.
    /// If the computer sleeps, the lid closes or Windows restarts for an
    /// update, the desk stops while the user goes on believing it runs.
    Heartbeat { at_ms: u64 },
    /// Status reply.
    Status(DeskStatus),
    /// Acknowledgement that a command was APPLIED.
    ///
    /// A command does not count as successful until this arrives. The desk
    /// sends one for every command (`aleph_desk_link`, invariant 1).
    Ack {
        /// `seq` of the acknowledged command.
        for_seq: u64,
        applied: bool,
        at_ms: u64,
        /// English fallback text: older phones that do not know the `reply`
        /// field show this. Newer phones render `reply` in their own language.
        detail: String,
        /// The localisable form of the result: a stable code plus numeric
        /// parameters. An older desk does not send it (`None`); an older phone
        /// ignores the field, which is why the version was not bumped.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reply: Option<AckReply>,
    },
}

/// The stable code and numeric parameters of an acknowledgement.
///
/// # Why not text
///
/// A sentence imposes the desk's language on the phone, and the phone can
/// neither translate it nor read the number inside it. A code plus numbers
/// lets each end render the result in its own language.
///
/// The wire form is flat: `{"code":"stopped","open":3}`. The code is a
/// `String`, NOT an enum: a desk that adds a new code must not make the whole
/// acknowledgement undecodable on a phone that does not know it. On an
/// unknown code the phone falls back to `detail`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AckReply {
    pub code: String,
    #[serde(flatten)]
    pub params: std::collections::BTreeMap<String, i64>,
}

/// Acknowledgement codes. The desk and the phone use the same constants.
pub mod ack_code {
    /// No new positions are opened; `open` open positions are still managed.
    pub const STOPPED: &str = "stopped";
    /// `closed` positions were closed.
    pub const CLOSED: &str = "closed";
    /// `closed` were closed and `pending` could not be priced; those close in
    /// the next round.
    pub const CLOSED_PARTIAL: &str = "closed_partial";
    /// Started; `futures` / `spot` / `pump` = 1 marks the bots that started.
    pub const STARTED: &str = "started";
    /// The daily loss stop is active; nothing starts until the next UTC day.
    pub const KILL_SWITCH_TRIPPED: &str = "kill_switch_tripped";
    /// There is no configured bot to start.
    pub const NOTHING_TO_START: &str = "nothing_to_start";
    /// The pump bot runs only on the "ambitious" level.
    pub const PUMP_NEEDS_AMBITIOUS: &str = "pump_needs_ambitious";
    /// The start did not happen for some other reason.
    pub const START_FAILED: &str = "start_failed";
}

impl AckReply {
    pub fn new(code: &str) -> Self {
        Self {
            code: code.to_string(),
            params: Default::default(),
        }
    }

    pub fn with(mut self, name: &str, value: i64) -> Self {
        self.params.insert(name.to_string(), value);
        self
    }

    pub fn param(&self, name: &str) -> i64 {
        self.params.get(name).copied().unwrap_or(0)
    }

    pub fn stopped(open: u64) -> Self {
        Self::new(ack_code::STOPPED).with("open", open as i64)
    }

    /// With `pending == 0` this is a plain "closed"; otherwise it STATES what
    /// remains. Saying "all closed" would hide the unpriced position from the
    /// user.
    pub fn closed(closed: u64, pending: u64) -> Self {
        if pending == 0 {
            Self::new(ack_code::CLOSED).with("closed", closed as i64)
        } else {
            Self::new(ack_code::CLOSED_PARTIAL)
                .with("closed", closed as i64)
                .with("pending", pending as i64)
        }
    }

    pub fn started(bots: &[BotSelector]) -> Self {
        let mut r = Self::new(ack_code::STARTED);
        for b in bots {
            if let Some(name) = b.started_param() {
                r = r.with(name, 1);
            }
        }
        r
    }

    /// English text: the `detail` field for older phones, and the phone UI
    /// where it has no translation yet, use this. An unknown code comes back
    /// as the code itself, never as an empty string.
    pub fn english(&self) -> String {
        let plural = |n: i64| if n == 1 { "" } else { "s" };
        match self.code.as_str() {
            ack_code::STOPPED => {
                let n = self.param("open");
                format!(
                    "not opening new positions; {n} open position{} still managed",
                    plural(n)
                )
            }
            ack_code::CLOSED => {
                let n = self.param("closed");
                format!("{n} position{} closed", plural(n))
            }
            ack_code::CLOSED_PARTIAL => {
                let (n, p) = (self.param("closed"), self.param("pending"));
                format!(
                    "{n} position{} closed, {p} could not be priced and will close next round",
                    plural(n)
                )
            }
            ack_code::STARTED => {
                let names: Vec<&str> = ["futures", "spot", "pump"]
                    .into_iter()
                    .filter(|b| self.param(b) == 1)
                    .collect();
                format!("started: {}", names.join(", "))
            }
            ack_code::KILL_SWITCH_TRIPPED => {
                "daily loss stop is active; cannot start until the next UTC day".into()
            }
            ack_code::NOTHING_TO_START => "no configured bot to start".into(),
            ack_code::PUMP_NEEDS_AMBITIOUS => "the pump bot runs only on the ambitious level".into(),
            ack_code::START_FAILED => "the bot could not be started".into(),
            other => other.to_string(),
        }
    }
}

impl BotSelector {
    /// Parameter name in the `started` acknowledgement (`All` is not a single
    /// bot).
    fn started_param(self) -> Option<&'static str> {
        match self {
            BotSelector::Futures => Some("futures"),
            BotSelector::Spot => Some("spot"),
            BotSelector::Pump => Some("pump"),
            BotSelector::All => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeskStatus {
    pub bots_running: Vec<String>,
    pub open_positions: u32,
    pub today_net_pct: f64,
    /// Setups not entered today and their reasons: the desktop's skip notes.
    pub skips_today: Vec<SkipNote>,
    /// Open positions as a share of the account (%). NOT a win rate.
    pub exposure_pct: f64,
    /// How many independent bets these positions behave like.
    pub effective_bets: f64,
    pub kill_switch_tripped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkipNote {
    pub reason: String,
    pub count: u32,
}

/// The signed envelope on the wire. The relay carries it as an OPAQUE value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u8,
    pub desk_id: String,
    /// The sender's SESSION stamp; tells a restart apart from a replay attack.
    /// Details: `ReplayGuard`.
    pub epoch: u64,
    /// Per-sender counter, monotonically increasing within a session.
    pub seq: u64,
    pub at_ms: u64,
    /// Serialised payload (Command or DeskMessage).
    pub payload: String,
    /// HMAC-SHA256 over the canonical bytes, base64.
    pub sig: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    BadPairingString,
    VersionMismatch {
        got: u8,
        want: u8,
    },
    BadSignature,
    /// The envelope is outside the accepted clock window.
    Stale {
        at_ms: u64,
        now_ms: u64,
    },
    /// The envelope is behind, or equal to, the last (epoch, seq) pair seen
    /// from this sender: a replay attempt.
    Replay {
        epoch: u64,
        seq: u64,
        last_epoch: u64,
        last_seq: u64,
    },
    Malformed(String),
    /// The envelope is addressed to another desk (`desk_id` is not this
    /// desk's).
    WrongDesk,
    /// The pairing key expired before it was ever used, or the pairing was
    /// cancelled: no envelope signed with this key is accepted any more.
    KeyRevoked,
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadPairingString => write!(f, "invalid pairing string"),
            Self::VersionMismatch { got, want } => {
                write!(f, "protocol version mismatch: got {got}, expected {want}")
            }
            Self::BadSignature => write!(f, "signature did not verify"),
            Self::Stale { at_ms, now_ms } => {
                write!(f, "envelope outside the time window (envelope {at_ms}, now {now_ms})")
            }
            Self::Replay {
                epoch,
                seq,
                last_epoch,
                last_seq,
            } => write!(
                f,
                "replay: received ({epoch}, {seq}), last seen ({last_epoch}, {last_seq})"
            ),
            Self::Malformed(m) => write!(f, "bozuk zarf: {m}"),
            Self::WrongDesk => write!(f, "envelope is for another desk"),
            Self::KeyRevoked => write!(f, "pairing key revoked"),
        }
    }
}

impl std::error::Error for LinkError {}

/// The canonical bytes that are signed.
///
/// Fields are joined in a fixed order with a separator. JSON serialisation is
/// NOT trusted: a change in field order or whitespace would silently break the
/// signature, and the error would read "signature mismatch" and hide the real
/// cause.
fn canonical_bytes(
    v: u8,
    desk_id: &str,
    epoch: u64,
    seq: u64,
    at_ms: u64,
    payload: &str,
) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&[v]);
    b.push(0x1f);
    b.extend_from_slice(desk_id.as_bytes());
    b.push(0x1f);
    // The epoch is inside the signature too. Outside it, someone in the middle
    // could raise the epoch and pass a recorded envelope off as a "new session".
    b.extend_from_slice(&epoch.to_be_bytes());
    b.push(0x1f);
    b.extend_from_slice(&seq.to_be_bytes());
    b.push(0x1f);
    b.extend_from_slice(&at_ms.to_be_bytes());
    b.push(0x1f);
    b.extend_from_slice(payload.as_bytes());
    b
}

fn sign(key: &PairingKey, bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::STANDARD, Engine};
    let mut mac = HmacSha256::new_from_slice(&key.0).expect("HMAC accepts any key length");
    mac.update(bytes);
    STANDARD.encode(mac.finalize().into_bytes())
}

/// Puts a payload into a signed envelope.
///
/// `epoch` is the stamp of the sender's CURRENT session; `seq` is the counter
/// within that session. Carrying both keeps a restarted end from being taken
/// for a replay attacker (see `ReplayGuard`).
pub fn seal<T: Serialize>(
    key: &PairingKey,
    desk_id: &str,
    epoch: u64,
    seq: u64,
    at_ms: u64,
    payload: &T,
) -> Result<Envelope, LinkError> {
    let payload =
        serde_json::to_string(payload).map_err(|e| LinkError::Malformed(e.to_string()))?;
    let sig = sign(
        key,
        &canonical_bytes(PROTOCOL_VERSION, desk_id, epoch, seq, at_ms, &payload),
    );
    Ok(Envelope {
        v: PROTOCOL_VERSION,
        desk_id: desk_id.to_string(),
        epoch,
        seq,
        at_ms,
        payload,
        sig,
    })
}

/// Receiver-side replay state. Kept per sender.
///
/// # Why a counter alone is not enough
///
/// Neither end persists its counter; both start it from zero. Judged by `seq`
/// alone, every heartbeat a restarted desk sends (Windows update, lid, crash)
/// has `seq <= last` and counts as a REPLAY, and the phone shows "your desk
/// went silent" forever: the protocol would produce the very failure the
/// heartbeat exists to catch. A restarted phone is worse: the desk would drop
/// every command, "close all" included.
///
/// # Why an epoch rather than a counter on disk
///
/// The receiver **cannot tell** a restarted peer from a replay attacker. So
/// the distinction is not left to the receiver's guess: the sender DECLARES
/// it, signed. `epoch` is part of the canonical bytes, so nobody without the
/// key can produce a new epoch. A counter written to disk would bring the same
/// failure back: if the file is deleted or corrupted (a moved profile, a
/// reinstall, the phone clearing app data) the counter resets and the line
/// locks up, this time with no way to recover. It would also put file I/O into
/// two pure protocol crates.
///
/// Rule: the epoch only moves FORWARD. A recorded envelope's epoch always lies
/// in the past, so replaying an old session stays impossible. The epoch is the
/// sender's session start time, so the clock assumption here is no stronger
/// than the one `MAX_CLOCK_SKEW_MS` already makes.
#[derive(Debug, Default, Clone)]
pub struct ReplayGuard {
    /// The last accepted (epoch, seq).
    last: Option<(u64, u64)>,
}

impl ReplayGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advanced only by accepted envelopes.
    fn accept(&mut self, epoch: u64, seq: u64) -> Result<(), LinkError> {
        if let Some((last_epoch, last_seq)) = self.last {
            // A new session (larger epoch) may restart the counter from zero;
            // within one session the counter MUST advance; an older session is
            // never accepted.
            let newer = epoch > last_epoch || (epoch == last_epoch && seq > last_seq);
            if !newer {
                return Err(LinkError::Replay {
                    epoch,
                    seq,
                    last_epoch,
                    last_seq,
                });
            }
        }
        self.last = Some((epoch, seq));
        Ok(())
    }

    pub fn last_seq(&self) -> Option<u64> {
        self.last.map(|(_, seq)| seq)
    }

    pub fn last_epoch(&self) -> Option<u64> {
        self.last.map(|(epoch, _)| epoch)
    }
}

/// Opens an envelope: checks the version, the signature, the clock window and
/// the replay counter.
///
/// Order matters: no field is trusted before the signature is verified.
pub fn open<T: for<'de> Deserialize<'de>>(
    key: &PairingKey,
    guard: &mut ReplayGuard,
    now_ms: u64,
    env: &Envelope,
) -> Result<T, LinkError> {
    if env.v != PROTOCOL_VERSION {
        return Err(LinkError::VersionMismatch {
            got: env.v,
            want: PROTOCOL_VERSION,
        });
    }

    let expected = sign(
        key,
        &canonical_bytes(
            env.v,
            &env.desk_id,
            env.epoch,
            env.seq,
            env.at_ms,
            &env.payload,
        ),
    );
    // Constant-time comparison: `==` leaks through timing how much of the
    // signature matched.
    if expected.as_bytes().ct_eq(env.sig.as_bytes()).unwrap_u8() != 1 {
        return Err(LinkError::BadSignature);
    }

    let skew = now_ms.abs_diff(env.at_ms);
    if skew > MAX_CLOCK_SKEW_MS {
        return Err(LinkError::Stale {
            at_ms: env.at_ms,
            now_ms,
        });
    }

    // The payload is decoded BEFORE the counter advances. Both ends sign with
    // the same key, so the desk's own heartbeat also carries a valid signature;
    // if the relay reflects it back to the desk the signature holds, but the
    // payload is not a `Command`. Were the counter advanced first, the
    // reflected envelope (desk epoch) could push the counter past the phone's
    // epoch, and EVERY later phone command, "close all" included, would be
    // dropped as a replay: the relay could silence the kill switch without
    // forging a signature. An envelope going the wrong way is rejected as
    // `Malformed` and leaves the counter untouched.
    let payload: T =
        serde_json::from_str(&env.payload).map_err(|e| LinkError::Malformed(e.to_string()))?;

    guard.accept(env.epoch, env.seq)?;

    Ok(payload)
}

/// The decoded QR text the phone reads.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedPairing {
    pub relay_url: String,
    pub desk_id: String,
    pub key_str: String,
}

pub fn parse_qr(text: &str) -> Result<ScannedPairing, LinkError> {
    let rest = text
        .strip_prefix("alephedge://pair?")
        .ok_or(LinkError::BadPairingString)?;
    let (mut relay, mut desk, mut k) = (None, None, None);
    for part in rest.split('&') {
        let (name, value) = part.split_once('=').ok_or(LinkError::BadPairingString)?;
        match name {
            "relay" => relay = Some(value.to_string()),
            "desk" => desk = Some(value.to_string()),
            "k" => k = Some(value.to_string()),
            // An unknown field is NOT silently swallowed: an older phone that
            // ignores a field added later is running half of the protocol.
            _ => return Err(LinkError::BadPairingString),
        }
    }
    match (relay, desk, k) {
        (Some(relay_url), Some(desk_id), Some(key_str))
            if !relay_url.is_empty() && !desk_id.is_empty() && !key_str.is_empty() =>
        {
            // Check HERE that the key really decodes. Reporting "paired" with
            // a broken key and failing on the first command moves the error to
            // the wrong place.
            PairingKey::from_pairing_string(&key_str).map_err(|_| LinkError::BadPairingString)?;
            Ok(ScannedPairing {
                relay_url,
                desk_id,
                key_str,
            })
        }
        _ => Err(LinkError::BadPairingString),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> PairingKey {
        PairingKey::from_bytes([7u8; 32])
    }

    const NOW: u64 = 1_786_700_000_000;
    /// The sender's session stamp. In production, the ms the session started.
    const EPOCH: u64 = 1_786_699_000_000;

    #[test]
    fn command_round_trips_unchanged() {
        let k = key();
        let cmd = Command::CloseAll {
            bot: BotSelector::Futures,
        };
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &cmd).unwrap();
        let mut g = ReplayGuard::new();
        let got: Command = open(&k, &mut g, NOW, &env).unwrap();
        assert_eq!(got, cmd);
    }

    /// The relay's reason to exist: it CARRIES messages and cannot FORGE them.
    #[test]
    fn relay_cannot_forge_a_command() {
        let desk_key = key();
        let relay_key = PairingKey::from_bytes([9u8; 32]); // any key the relay could hold

        let forged = seal(
            &relay_key,
            "desk-1",
            EPOCH,
            1,
            NOW,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        )
        .unwrap();

        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&desk_key, &mut g, NOW, &forged);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
        // A rejected envelope must NOT advance the counter, or an attacker
        // could burn counter values and make legitimate commands drop.
        assert_eq!(g.last_seq(), None);
    }

    #[test]
    fn one_changed_payload_bit_fails_the_signature() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        // An attempt to turn "show status" into "close all".
        env.payload = serde_json::to_string(&Command::CloseAll {
            bot: BotSelector::All,
        })
        .unwrap();
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }

    #[test]
    fn changed_desk_id_fails_the_signature() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        // An attempt to redirect the command to SOMEONE ELSE's desk.
        env.desk_id = "desk-2".into();
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }

    #[test]
    fn recorded_envelope_cannot_be_replayed() {
        let k = key();
        let env = seal(
            &k,
            "desk-1",
            EPOCH,
            5,
            NOW,
            &Command::StopOpening {
                bot: BotSelector::All,
            },
        )
        .unwrap();
        let mut g = ReplayGuard::new();
        let _: Command = open(&k, &mut g, NOW, &env).unwrap();

        // The same envelope again: valid signature, valid clock, old counter.
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(
            r.unwrap_err(),
            LinkError::Replay {
                epoch: EPOCH,
                seq: 5,
                last_epoch: EPOCH,
                last_seq: 5,
            }
        );
    }

    #[test]
    fn old_envelope_cannot_be_replayed_days_later() {
        let k = key();
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        let mut g = ReplayGuard::new();
        let later = NOW + 48 * 3600 * 1000;
        let r: Result<Command, _> = open(&k, &mut g, later, &env);
        assert!(matches!(r.unwrap_err(), LinkError::Stale { .. }));
    }

    #[test]
    fn reasonable_clock_skew_is_accepted() {
        let k = key();
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        let mut g = ReplayGuard::new();
        // The phone's clock is 60 s ahead.
        let r: Result<Command, _> = open(&k, &mut g, NOW - 60_000, &env);
        assert!(r.is_ok(), "a reasonable skew must not be rejected: {r:?}");
    }

    #[test]
    fn version_mismatch_is_never_silently_interpreted() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        env.v = PROTOCOL_VERSION + 1;
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(
            r.unwrap_err(),
            LinkError::VersionMismatch {
                got: PROTOCOL_VERSION + 1,
                want: PROTOCOL_VERSION
            }
        );
    }

    /// "Stop" and "close all" are SEPARATE commands and neither serialises as
    /// the other (see `Command::StopOpening`).
    #[test]
    fn stop_and_close_all_are_separate_commands() {
        let stop = serde_json::to_string(&Command::StopOpening {
            bot: BotSelector::All,
        })
        .unwrap();
        let close = serde_json::to_string(&Command::CloseAll {
            bot: BotSelector::All,
        })
        .unwrap();
        assert_ne!(stop, close);
        assert!(stop.contains("stop_opening"), "{stop}");
        assert!(close.contains("close_all"), "{close}");
    }

    #[test]
    fn pairing_key_round_trips() {
        let k = PairingKey::generate();
        let s = k.to_pairing_string();
        let k2 = PairingKey::from_pairing_string(&s).unwrap();
        assert_eq!(k.0, k2.0);
        // Short enough to print into a QR.
        assert!(s.len() < 64, "pairing string is {} characters", s.len());
    }

    #[test]
    fn key_never_reaches_a_log_line() {
        let k = PairingKey::from_bytes([42u8; 32]);
        let s = format!("{k:?}");
        assert!(!s.contains("42"), "key leaked into Debug output: {s}");
        // The Debug form prints `<redacted>` in place of the bytes.
        assert!(s.contains("<redacted>"));
    }

    /// A restarted peer starts its counter from zero. Without the epoch every
    /// heartbeat would be rejected as a replay (see `ReplayGuard`).
    ///
    /// The restarted peer is ACCEPTED; a real replay is still rejected.
    #[test]
    fn restarted_peer_is_accepted_real_replay_is_rejected() {
        let k = key();
        let mut g = ReplayGuard::new();

        // First session: two beats.
        let a = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        let b = seal(&k, "desk-1", EPOCH, 2, NOW, &Command::Status).unwrap();
        let _: Command = open(&k, &mut g, NOW, &a).unwrap();
        let _: Command = open(&k, &mut g, NOW, &b).unwrap();

        // The desk restarted: new epoch, counter back at 1.
        let new_epoch = EPOCH + 1;
        let c = seal(&k, "desk-1", new_epoch, 1, NOW, &Command::Status).unwrap();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &c);
        assert!(r.is_ok(), "restarted peer was rejected: {r:?}");

        // The same envelope again: still a replay.
        assert!(matches!(
            open::<Command>(&k, &mut g, NOW, &c).unwrap_err(),
            LinkError::Replay { .. }
        ));

        // And an envelope recorded in the PREVIOUS session: older epoch, rejected.
        let old = seal(&k, "desk-1", EPOCH, 99, NOW, &Command::Status).unwrap();
        assert!(
            matches!(
                open::<Command>(&k, &mut g, NOW, &old).unwrap_err(),
                LinkError::Replay { .. }
            ),
            "an old session's envelope was accepted"
        );
    }

    /// The epoch is INSIDE the signature (see `canonical_bytes`).
    #[test]
    fn tampered_epoch_fails_the_signature() {
        let k = key();
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &Command::Status).unwrap();
        env.epoch = EPOCH + 5_000;
        let mut g = ReplayGuard::new();
        let r: Result<Command, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }

    /// A desk heartbeat the relay reflects back to the desk carries a valid
    /// signature but must not advance the counter (see `open`).
    #[test]
    fn reflected_desk_envelope_does_not_advance_the_counter() {
        let k = key();
        let mut desk_guard = ReplayGuard::new();
        // The desk started AFTER the phone: the desk's epoch is larger.
        let desk_epoch = EPOCH + 10_000;
        let beat = seal(
            &k,
            "desk-1",
            desk_epoch,
            1,
            NOW,
            &DeskMessage::Heartbeat { at_ms: NOW },
        )
        .unwrap();
        let r: Result<Command, _> = open(&k, &mut desk_guard, NOW, &beat);
        assert!(matches!(r.unwrap_err(), LinkError::Malformed(_)));
        assert_eq!(desk_guard.last_epoch(), None, "the reflection advanced the counter");

        // The phone's real command must still pass.
        let close = seal(
            &k,
            "desk-1",
            EPOCH,
            1,
            NOW,
            &Command::CloseAll {
                bot: BotSelector::All,
            },
        )
        .unwrap();
        let got: Command = open(&k, &mut desk_guard, NOW, &close).unwrap();
        assert_eq!(
            got,
            Command::CloseAll {
                bot: BotSelector::All
            }
        );
    }

    #[test]
    fn ack_round_trips() {
        let k = key();
        let msg = DeskMessage::Ack {
            for_seq: 5,
            applied: true,
            at_ms: NOW,
            detail: AckReply::closed(6, 0).english(),
            reply: Some(AckReply::closed(6, 0)),
        };
        let env = seal(&k, "desk-1", EPOCH, 1, NOW, &msg).unwrap();
        let mut g = ReplayGuard::new();
        let got: DeskMessage = open(&k, &mut g, NOW, &env).unwrap();
        assert_eq!(got, msg);
    }

    /// The wire form is flat and numeric: the phone reads the code and the
    /// number, not a sentence.
    #[test]
    fn ack_code_serialises_flat_and_numeric() {
        let v = serde_json::to_value(AckReply::stopped(3)).unwrap();
        assert_eq!(v, serde_json::json!({"code": "stopped", "open": 3}));
        let back: AckReply = serde_json::from_value(v).unwrap();
        assert_eq!(back, AckReply::stopped(3));
    }

    /// An older desk sends no `reply`; a newer phone still decodes (None).
    /// An older phone does not know `reply`; the field is ignored, `detail`
    /// remains.
    #[test]
    fn ack_code_is_compatible_with_older_ends() {
        let old_json = r#"{"msg":"ack","for_seq":1,"applied":true,"at_ms":5,"detail":"x"}"#;
        match serde_json::from_str::<DeskMessage>(old_json).unwrap() {
            DeskMessage::Ack { reply, detail, .. } => {
                assert_eq!(reply, None);
                assert_eq!(detail, "x");
            }
            other => panic!("{other:?}"),
        }

        #[derive(Deserialize)]
        #[serde(tag = "msg", rename_all = "snake_case")]
        #[allow(dead_code)]
        enum OldMessage {
            Ack { for_seq: u64, applied: bool, at_ms: u64, detail: String },
        }
        let new_json = serde_json::to_string(&DeskMessage::Ack {
            for_seq: 1,
            applied: true,
            at_ms: 5,
            detail: AckReply::stopped(2).english(),
            reply: Some(AckReply::stopped(2)),
        })
        .unwrap();
        let OldMessage::Ack { detail, .. } = serde_json::from_str(&new_json).unwrap();
        assert!(detail.contains('2'), "{detail}");
    }

    #[test]
    fn ack_english_text() {
        assert_eq!(
            AckReply::stopped(1).english(),
            "not opening new positions; 1 open position still managed"
        );
        assert_eq!(AckReply::closed(6, 0).english(), "6 positions closed");
        assert_eq!(
            AckReply::closed(4, 2).english(),
            "4 positions closed, 2 could not be priced and will close next round"
        );
        assert_eq!(AckReply::closed(4, 2).code, ack_code::CLOSED_PARTIAL);
        assert_eq!(
            AckReply::started(&[BotSelector::Futures, BotSelector::Pump]).english(),
            "started: futures, pump"
        );
        // An unknown code never produces empty text.
        assert_eq!(AckReply::new("future_code").english(), "future_code");
    }

    /// The ack code is INSIDE the signed payload: tampering breaks the
    /// signature.
    #[test]
    fn ack_code_is_inside_the_signature() {
        let k = key();
        let msg = DeskMessage::Ack {
            for_seq: 1,
            applied: true,
            at_ms: NOW,
            detail: AckReply::stopped(8).english(),
            reply: Some(AckReply::stopped(8)),
        };
        let mut env = seal(&k, "desk-1", EPOCH, 1, NOW, &msg).unwrap();
        env.payload = env.payload.replace("\"open\":8", "\"open\":0");
        let mut g = ReplayGuard::new();
        let r: Result<DeskMessage, _> = open(&k, &mut g, NOW, &env);
        assert_eq!(r.unwrap_err(), LinkError::BadSignature);
    }
}
