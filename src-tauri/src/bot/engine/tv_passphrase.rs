//! The TradingView bot's optional passphrase, as the bot keeps it.
//!
//! Since 0.3.20 it is an Argon2id hash (`TvSettings::passphrase_hash`):
//! `argon2id$v=19$m=<KiB>,t=<passes>,p=<lanes>$<salt>$<hash>`, salt and hash
//! in unpadded base64. The cost is stored with the hash, so a later release
//! can raise it without breaking saved bots.
//!
//! 0.3.19 kept hex SHA-256 of `salt:passphrase` (`passphrase_sha256` and
//! `passphrase_salt`). Such a bot keeps working: an alert whose passphrase
//! matches the old hash is accepted (`Verdict::LegacyMatch`), and the caller
//! then stores the Argon2id hash in its place (`upgrade_legacy`). Saving a
//! new passphrase in the form writes the new format directly.
//!
//! Every comparison is constant time. A stored hash that does not parse, or
//! whose cost is outside the accepted range, refuses every alert (fail
//! closed): the config file is not authenticated, and a tampered cost must
//! not become a large allocation on the alert path.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD_NO_PAD as B64;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::super::model::TvSettings;

/// Argon2id cost written by this release: 19 MiB, 2 passes, 1 lane (the
/// OWASP minimum for Argon2id). Tens of milliseconds per alert in a release
/// build.
pub const COST: Cost = Cost { m: 19_456, t: 2, p: 1 };

/// Accepted range for a cost read back from disk.
const M_MIN: u32 = 8_192;
const M_MAX: u32 = 65_536;
const T_MAX: u32 = 4;
const P_MAX: u32 = 4;

const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const TAG: &str = "argon2id";
const VERSION_FIELD: &str = "v=19";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cost {
    /// Memory, KiB.
    pub m: u32,
    /// Passes.
    pub t: u32,
    /// Lanes.
    pub p: u32,
}

impl Cost {
    fn in_range(&self) -> bool {
        (M_MIN..=M_MAX).contains(&self.m) && (1..=T_MAX).contains(&self.t) && (1..=P_MAX).contains(&self.p)
    }
}

/// What an alert's passphrase does against the bot's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// The bot has no passphrase: any alert passes this check.
    NoPassphrase,
    /// Matches the Argon2id hash.
    Match,
    /// Matches a 0.3.19 SHA-256 hash: accepted, and the caller rehashes.
    LegacyMatch,
    /// Missing, wrong, or the stored hash is unreadable.
    Refused,
}

impl Verdict {
    pub fn opens(&self) -> bool {
        !matches!(self, Verdict::Refused)
    }
}

fn derive(passphrase: &str, salt: &[u8], cost: Cost) -> Result<[u8; HASH_LEN], String> {
    let params = Params::new(cost.m, cost.t, cost.p, Some(HASH_LEN)).map_err(|_| "tvPassphraseHashFailed".to_string())?;
    let mut out = [0u8; HASH_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|_| "tvPassphraseHashFailed".to_string())?;
    Ok(out)
}

fn encode(cost: Cost, salt: &[u8], hash: &[u8]) -> String {
    format!("{TAG}${VERSION_FIELD}$m={},t={},p={}${}${}", cost.m, cost.t, cost.p, B64.encode(salt), B64.encode(hash))
}

fn decode(stored: &str) -> Option<(Cost, Vec<u8>, Vec<u8>)> {
    let mut parts = stored.split('$');
    if parts.next()? != TAG || parts.next()? != VERSION_FIELD {
        return None;
    }
    let mut cost = Cost { m: 0, t: 0, p: 0 };
    for kv in parts.next()?.split(',') {
        let (k, v) = kv.split_once('=')?;
        let v: u32 = v.parse().ok()?;
        match k {
            "m" => cost.m = v,
            "t" => cost.t = v,
            "p" => cost.p = v,
            _ => return None,
        }
    }
    let salt = B64.decode(parts.next()?).ok()?;
    let hash = B64.decode(parts.next()?).ok()?;
    if parts.next().is_some() || !cost.in_range() || !(SALT_LEN..=64).contains(&salt.len()) || hash.len() != HASH_LEN {
        return None;
    }
    Some((cost, salt, hash))
}

/// The stored form of `passphrase` (already trimmed by the caller), with a
/// fresh random salt.
pub fn hash(passphrase: &str) -> Result<String, String> {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    let out = derive(passphrase, &salt, COST)?;
    Ok(encode(COST, &salt, &out))
}

/// Whether `passphrase` matches a stored Argon2id hash (constant time).
pub fn verify(stored: &str, passphrase: &str) -> bool {
    let Some((cost, salt, want)) = decode(stored) else {
        return false;
    };
    match derive(passphrase, &salt, cost) {
        Ok(got) => bool::from(got.as_slice().ct_eq(want.as_slice())),
        Err(_) => false,
    }
}

/// The 0.3.19 format: hex SHA-256 of `salt:passphrase`. Read only.
pub fn legacy_digest(salt: &str, passphrase: &str) -> String {
    let digest = Sha256::digest(format!("{salt}:{passphrase}").as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Checks an alert's `passphrase` against the bot. The Argon2id hash wins
/// when both formats are present; the text is trimmed as the form trims it.
pub fn check(tv: &TvSettings, given: Option<&str>) -> Verdict {
    if let Some(stored) = tv.passphrase_hash.as_deref() {
        return match given {
            Some(g) if verify(stored, g.trim()) => Verdict::Match,
            _ => Verdict::Refused,
        };
    }
    if let (Some(want), Some(salt)) = (tv.passphrase_sha256.as_deref(), tv.passphrase_salt.as_deref()) {
        let Some(g) = given else {
            return Verdict::Refused;
        };
        let got = legacy_digest(salt, g.trim());
        return if bool::from(got.as_bytes().ct_eq(want.as_bytes())) {
            Verdict::LegacyMatch
        } else {
            Verdict::Refused
        };
    }
    Verdict::NoPassphrase
}

/// Replaces a 0.3.19 hash with `new_hash`, only if `tv` still holds the
/// legacy hash the alert matched (`matched_sha256`): a settings save made in
/// between keeps what it saved. Returns whether `tv` changed.
pub fn upgrade_legacy(tv: &mut TvSettings, matched_sha256: &str, new_hash: String) -> bool {
    if tv.passphrase_hash.is_some() || tv.passphrase_sha256.as_deref() != Some(matched_sha256) {
        return false;
    }
    tv.passphrase_hash = Some(new_hash);
    tv.passphrase_sha256 = None;
    tv.passphrase_salt = None;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A random test passphrase, made at run time (no literal secrets).
    fn random_text() -> String {
        let mut b = [0u8; 12];
        OsRng.fill_bytes(&mut b);
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// A 0.3.19 bot's settings for `passphrase`, as 0.3.19 wrote them.
    fn legacy_tv(passphrase: &str) -> TvSettings {
        let salt = format!("{:032x}", rand::random::<u128>());
        TvSettings {
            passphrase_sha256: Some(legacy_digest(&salt, passphrase)),
            passphrase_salt: Some(salt),
            ..TvSettings::default()
        }
    }

    #[test]
    fn a_new_hash_verifies_only_its_own_passphrase() {
        let pass = random_text();
        let stored = hash(&pass).unwrap();
        assert!(stored.starts_with("argon2id$v=19$m=19456,t=2,p=1$"), "{stored}");
        assert!(!stored.contains(&pass));
        assert!(verify(&stored, &pass));
        assert!(!verify(&stored, &format!("{pass}x")));
        assert!(!verify(&stored, ""));
        // A fresh salt per hash.
        assert_ne!(stored, hash(&pass).unwrap());
    }

    #[test]
    fn an_unreadable_or_out_of_range_hash_refuses_everything() {
        let pass = random_text();
        let stored = hash(&pass).unwrap();
        let swap = |from: &str, to: &str| stored.replacen(from, to, 1);
        for bad in [
            String::new(),
            swap("argon2id", "argon2i"),
            swap("v=19", "v=16"),
            swap("m=19456", "m=4000000"), // a tampered cost is never allocated
            swap("m=19456", "m=1024"),
            swap("t=2", "t=99"),
            swap("p=1", "p=0"),
            format!("{stored}$extra"),
            stored[..stored.len() - 4].to_string(),
        ] {
            assert!(!verify(&bad, &pass), "{bad}");
            let tv = TvSettings { passphrase_hash: Some(bad.clone()), ..TvSettings::default() };
            assert_eq!(check(&tv, Some(&pass)), Verdict::Refused, "{bad}");
        }
    }

    #[test]
    fn the_alert_check_covers_none_new_and_legacy() {
        let pass = random_text();
        assert_eq!(check(&TvSettings::default(), None), Verdict::NoPassphrase);
        assert_eq!(check(&TvSettings::default(), Some(&pass)), Verdict::NoPassphrase);
        let tv = TvSettings { passphrase_hash: Some(hash(&pass).unwrap()), ..TvSettings::default() };
        assert_eq!(check(&tv, Some(&pass)), Verdict::Match);
        assert_eq!(check(&tv, Some(&format!("  {pass} "))), Verdict::Match, "trimmed as the form trims");
        assert_eq!(check(&tv, Some(&random_text())), Verdict::Refused);
        assert_eq!(check(&tv, None), Verdict::Refused, "a set passphrase must be sent");
        let old = legacy_tv(&pass);
        assert_eq!(check(&old, Some(&pass)), Verdict::LegacyMatch);
        assert_eq!(check(&old, Some(&random_text())), Verdict::Refused);
        assert_eq!(check(&old, None), Verdict::Refused);
        assert!(Verdict::LegacyMatch.opens() && Verdict::NoPassphrase.opens() && !Verdict::Refused.opens());
    }

    // A bot saved by 0.3.19 (salted SHA-256) keeps opening with its
    // passphrase; the first matching alert moves it to Argon2id.
    #[test]
    fn a_0319_passphrase_keeps_working_and_is_rehashed_on_the_first_match() {
        let pass = random_text();
        let old = legacy_tv(&pass);
        // The settings as 0.3.19 stored them, read by this build.
        let json = serde_json::to_string(&old).unwrap();
        assert!(json.contains("\"passphraseSha256\"") && !json.contains("passphraseHash"), "{json}");
        let mut tv: TvSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(check(&tv, Some(&pass)), Verdict::LegacyMatch);
        let matched = tv.passphrase_sha256.clone().unwrap();
        assert!(upgrade_legacy(&mut tv, &matched, hash(&pass).unwrap()));
        assert!(tv.passphrase_sha256.is_none() && tv.passphrase_salt.is_none());
        assert_eq!(check(&tv, Some(&pass)), Verdict::Match);
        assert_eq!(check(&tv, Some(&random_text())), Verdict::Refused);
        let saved = serde_json::to_string(&tv).unwrap();
        assert!(saved.contains("\"passphraseHash\":\"argon2id$") && !saved.contains("passphraseSha256"), "{saved}");
        // Already upgraded: a second upgrade is a no-op.
        assert!(!upgrade_legacy(&mut tv, &matched, hash(&pass).unwrap()));
    }

    #[test]
    fn an_upgrade_never_overwrites_a_passphrase_saved_in_between() {
        let pass = random_text();
        let alert_saw = legacy_tv(&pass);
        let matched = alert_saw.passphrase_sha256.clone().unwrap();
        // The user saved a different passphrase (0.3.19 format here, so only
        // the hash differs) before the upgrade ran.
        let mut now = legacy_tv(&random_text());
        let before = now.clone();
        assert!(!upgrade_legacy(&mut now, &matched, hash(&pass).unwrap()));
        assert_eq!(now, before);
        // Or cleared it.
        let mut cleared = TvSettings::default();
        assert!(!upgrade_legacy(&mut cleared, &matched, hash(&pass).unwrap()));
        assert_eq!(cleared, TvSettings::default());
    }

    #[test]
    fn the_cost_is_inside_the_accepted_range() {
        assert!(COST.in_range());
    }
}
