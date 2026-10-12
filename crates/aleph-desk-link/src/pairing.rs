//! Pairing by QR.
//!
//! # Why so narrow
//!
//! The QR code contains the pairing key. A QR left on screen is therefore a
//! **standing credential**: anyone who sees it can stop that desk, start it
//! and close its positions. A photo over the shoulder is enough.
//!
//! So a pairing session:
//!   - is **short-lived** (`PAIRING_TTL_MS`),
//!   - is **single-use**: once a phone pairs, the QR dies,
//!   - and dies **visibly** when it expires; it does not stay on screen
//!     looking valid.
//!
//! The last point guards against a whole failure class: something that turns
//! invalid without showing it goes on being trusted.
//!
//! # Relay contract
//!
//! Two limits this pairing relies on are enforced by the relay, not by the
//! desk:
//!   - the relay accepts a phone for a desk only while that desk is connected;
//!   - it admits at most one phone per pairing, and only a phone that belongs
//!     to the desk's owner.
//!
//! The desk's own single-use check (`PairingSession::complete`) does not stop
//! a second phone that read the same QR within `PAIRING_TTL_MS`.

use aleph_link::PairingKey;

/// How long a QR stays valid (ms). Two minutes is enough to take out the
/// phone and scan; it does not outlive the user stepping away from the desk.
pub const PAIRING_TTL_MS: u64 = 120_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingError {
    /// The QR has expired.
    Expired,
    /// This QR has already paired with a phone.
    AlreadyUsed,
    /// The text read does not belong to this desk, or is corrupt.
    Invalid,
}

/// A stable error code: the desktop UI shows it in the user's language under
/// `errors.*`. A sentence here, in any language, would show a single-language
/// error in an eight-language UI.
impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expired => write!(f, "pairingExpired"),
            Self::AlreadyUsed => write!(f, "pairingUsed"),
            Self::Invalid => write!(f, "pairingInvalid"),
        }
    }
}

impl std::error::Error for PairingError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Used,
}

/// A pairing session opened on the desktop.
pub struct PairingSession {
    desk_id: String,
    relay_url: String,
    key: PairingKey,
    created_ms: u64,
    state: State,
}

impl PairingSession {
    pub fn begin(desk_id: impl Into<String>, relay_url: impl Into<String>, now_ms: u64) -> Self {
        Self {
            desk_id: desk_id.into(),
            relay_url: relay_url.into(),
            key: PairingKey::generate(),
            created_ms: now_ms,
            state: State::Pending,
        }
    }

    /// The text printed into the QR.
    ///
    /// `alephedge://pair?relay=<url>&desk=<id>&k=<key>`
    ///
    /// An expired or used session **produces no text**. Leaving a dead QR on
    /// screen shows the user something that does not work as if it did.
    pub fn qr_payload(&self, now_ms: u64) -> Result<String, PairingError> {
        self.check(now_ms)?;
        Ok(format!(
            "alephedge://pair?relay={}&desk={}&k={}",
            self.relay_url,
            self.desk_id,
            self.key.to_pairing_string()
        ))
    }

    fn check(&self, now_ms: u64) -> Result<(), PairingError> {
        if self.state == State::Used {
            return Err(PairingError::AlreadyUsed);
        }
        if now_ms.saturating_sub(self.created_ms) > PAIRING_TTL_MS {
            return Err(PairingError::Expired);
        }
        Ok(())
    }

    /// Time left (ms). The UI shows it as a countdown: the user must see when
    /// the QR dies.
    pub fn remaining_ms(&self, now_ms: u64) -> u64 {
        PAIRING_TTL_MS.saturating_sub(now_ms.saturating_sub(self.created_ms))
    }

    /// A phone paired: closes the session and hands out the lasting key.
    ///
    /// The desktop calls this on the first verified envelope, which proves
    /// the sender HOLDS the key. The desktop does not hold the camera, so it
    /// has no other way to know when the phone read the QR.
    pub fn complete(&mut self, now_ms: u64) -> Result<PairingKey, PairingError> {
        self.check(now_ms)?;
        self.state = State::Used;
        Ok(self.key.clone())
    }

    /// The key for the desk's OWN relay session.
    ///
    /// Not to be confused with `complete`: this is not the pairing passing to
    /// the other side but the same desk opening its own connection. The relay
    /// accepts a phone only while the desk is connected (see "Relay contract"
    /// above), so the desk MUST connect with this key while the QR is still on
    /// screen, and the QR is not spent here. The only thing that spends the QR
    /// is a signed envelope from the other side.
    pub fn desk_session_key(&self) -> PairingKey {
        self.key.clone()
    }
}

// `ScannedPairing` and `parse_qr` live in the shared crate (aleph-link): the
// desktop produces the QR and the phone reads it. Kept here, the phone would
// have to pull in this desktop crate or write its own copy, and a second
// implementation is what the shared crate exists to prevent.
pub use aleph_link::{parse_qr, ScannedPairing};

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_786_700_000_000;

    #[test]
    fn qr_text_round_trips() {
        let s = PairingSession::begin("desk-1", "wss://ribqa.com/link", NOW);
        let text = s.qr_payload(NOW).unwrap();
        let got = parse_qr(&text).unwrap();
        assert_eq!(got.desk_id, "desk-1");
        assert_eq!(got.relay_url, "wss://ribqa.com/link");
        // The key read back must be the one the desk generated.
        let k = PairingKey::from_pairing_string(&got.key_str).unwrap();
        assert_eq!(k.to_pairing_string(), s.key.to_pairing_string());
    }

    /// A QR on screen is a standing credential. Once expired it produces NO
    /// text, so a dead QR never looks live.
    #[test]
    fn expired_qr_produces_no_text() {
        let s = PairingSession::begin("desk-1", "wss://x", NOW);
        assert!(s.qr_payload(NOW + PAIRING_TTL_MS).is_ok());
        assert_eq!(
            s.qr_payload(NOW + PAIRING_TTL_MS + 1).unwrap_err(),
            PairingError::Expired
        );
    }

    #[test]
    fn expired_qr_cannot_pair() {
        let mut s = PairingSession::begin("desk-1", "wss://x", NOW);
        assert_eq!(
            s.complete(NOW + PAIRING_TTL_MS + 1).unwrap_err(),
            PairingError::Expired
        );
    }

    /// Single use: once a phone pairs, the QR dies. If a second phone could
    /// get in with the same QR, so could whoever photographed it.
    #[test]
    fn qr_is_single_use() {
        let mut s = PairingSession::begin("desk-1", "wss://x", NOW);
        s.complete(NOW).unwrap();
        assert_eq!(s.complete(NOW).unwrap_err(), PairingError::AlreadyUsed);
        assert_eq!(s.qr_payload(NOW).unwrap_err(), PairingError::AlreadyUsed);
    }

    #[test]
    fn countdown_is_correct() {
        let s = PairingSession::begin("desk-1", "wss://x", NOW);
        assert_eq!(s.remaining_ms(NOW), PAIRING_TTL_MS);
        assert_eq!(s.remaining_ms(NOW + 30_000), PAIRING_TTL_MS - 30_000);
        assert_eq!(s.remaining_ms(NOW + PAIRING_TTL_MS * 2), 0);
    }

    #[test]
    fn malformed_qr_is_rejected() {
        for bad in [
            "",
            "https://example.com",
            "alephedge://pair?",
            "alephedge://pair?relay=x",
            "alephedge://pair?relay=x&desk=d",
            "alephedge://pair?relay=&desk=d&k=abc",
        ] {
            assert!(parse_qr(bad).is_err(), "accepted: {bad:?}");
        }
    }

    /// A key that does not decode is caught at PAIRING time. Otherwise the
    /// phone reports "paired" and the error surfaces at the first command, in
    /// the wrong place.
    #[test]
    fn undecodable_key_is_caught_at_pairing() {
        let bad = "alephedge://pair?relay=wss://x&desk=d1&k=this-is-not-a-key";
        assert_eq!(
            parse_qr(bad).unwrap_err(),
            aleph_link::LinkError::BadPairingString
        );
    }

    /// An unknown field is not silently swallowed (see `aleph_link::parse_qr`).
    #[test]
    fn unknown_field_is_not_silently_swallowed() {
        let s = PairingSession::begin("d1", "wss://x", NOW);
        let text = format!("{}&new_field=1", s.qr_payload(NOW).unwrap());
        assert_eq!(
            parse_qr(&text).unwrap_err(),
            aleph_link::LinkError::BadPairingString
        );
    }

    /// Every session must generate a DIFFERENT key.
    #[test]
    fn every_session_generates_a_different_key() {
        let a = PairingSession::begin("d1", "wss://x", NOW);
        let b = PairingSession::begin("d1", "wss://x", NOW);
        assert_ne!(
            a.key.to_pairing_string(),
            b.key.to_pairing_string(),
            "two sessions generated the same key"
        );
    }

    #[test]
    fn errors_are_translatable_stable_codes() {
        // The UI localises these under `errors.*`: codes, not sentences.
        assert_eq!(PairingError::Expired.to_string(), "pairingExpired");
        assert_eq!(PairingError::AlreadyUsed.to_string(), "pairingUsed");
        assert_eq!(PairingError::Invalid.to_string(), "pairingInvalid");
    }
}
