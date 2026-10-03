//! Vault data types.
//!
//! Two clear tiers live here:
//!   * secret-bearing types (`ExchangeCredential`) held only in memory while the
//!     vault is unlocked, and
//!   * public metadata types (`CredentialMeta`, `VaultStatus`) that are the ONLY
//!     things ever sent to the UI. Secrets never cross the IPC boundary.

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// What an exchange API key is allowed to do. A withdraw-enabled key is a
/// standing risk the product warns about — trade-only keys are the norm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CredentialPermission {
    TradeOnly,
    WithdrawEnabled,
    /// Permission not declared by the user; treated cautiously.
    Unknown,
}

/// A full exchange credential — secrets included. Lives only in the unlocked
/// session's memory and in the sealed vault file; it is never serialized to the
/// UI.
///
/// Secret fields are wiped by this type's own `Drop` (below), not by the
/// session that happens to hold it. The session used to do the wiping, which
/// meant the copy handed out by `VaultManager::credential()` — one per live
/// order, per close, per reconcile pass — was never wiped at all.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeCredential {
    pub exchange_id: String,
    /// User label so one exchange can hold several keys (e.g. "main", "test").
    pub label: String,
    pub api_key: String,
    pub api_secret: String,
    /// Some exchanges (OKX, KuCoin) require a passphrase alongside the secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    pub permission: CredentialPermission,
    /// UNIX millis when the credential was added.
    pub added_at: u64,
    /// UNIX millis of the last key replacement (Settings, Renew key).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

impl ExchangeCredential {
    /// The safe, secret-free projection sent to the UI.
    pub fn to_meta(&self) -> CredentialMeta {
        CredentialMeta {
            exchange_id: self.exchange_id.clone(),
            label: self.label.clone(),
            permission: self.permission,
            has_passphrase: self.passphrase.is_some(),
            added_at: self.added_at,
            updated_at: self.updated_at,
        }
    }

    /// Wipe secret material. Called from `Drop`, so every copy is covered.
    fn zeroize_secrets(&mut self) {
        self.api_key.zeroize();
        self.api_secret.zeroize();
        if let Some(p) = self.passphrase.as_mut() {
            p.zeroize();
        }
    }
}

/// Wipes the key, secret and passphrase whenever a credential goes out of
/// scope — the session's copy on lock, and equally every short-lived clone the
/// order path takes out of it.
///
/// Having a `Drop` also makes moving a secret field *out* of a credential a
/// compile error, which is deliberate: `exchange::commands` used to do exactly
/// that (`Ok((cred.api_key, cred.api_secret))`), handing the caller two owned
/// `String`s with no owner left to wipe them. The borrow checker now refuses
/// that shape, so the fix cannot silently regress.
///
/// What this does NOT reach, and what no `Drop` here could:
///   * `reqwest`'s `HeaderValue` copy of the API key for `X-MBX-APIKEY`, and
///     whatever the HTTP stack buffers below it.
///   * `hmac` 0.12's internal key state in `sign_query`. That crate does not
///     zeroize on drop, so the padded secret blocks are left in freed memory.
///   * whatever `serde_json` allocated while parsing the IPC payload the key
///     arrived in, or the plaintext vault body on unlock — the vault's own
///     buffers are `Zeroizing`, the parser's intermediates are not reachable.
///
/// So the honest claim is "wiped where we own the allocation", not "wiped
/// everywhere".
impl Drop for ExchangeCredential {
    fn drop(&mut self) {
        self.zeroize_secrets();
    }
}

/// Secret-free credential summary — safe to send across IPC and render.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialMeta {
    pub exchange_id: String,
    pub label: String,
    pub permission: CredentialPermission,
    pub has_passphrase: bool,
    pub added_at: u64,
    pub updated_at: Option<u64>,
}

/// Argon2id cost parameters, stored in the vault header so a file sealed under
/// older params stays decryptable if defaults change later.
///
/// The salt itself is deliberately NOT here — it lives only in the OS
/// keychain (PRD §5.5: "OS keychain'e yalnızca salt referansı yazılır").
/// Keeping it out of `vault.edge` means the file alone, copied to another
/// device or user account, is not decryptable even with the correct
/// password — the keychain entry is a second required factor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfParams {
    /// Strong desktop defaults: 64 MiB memory, 3 passes, 1 lane.
    pub const M_COST: u32 = 65_536;
    pub const T_COST: u32 = 3;
    pub const P_COST: u32 = 1;

    // ---- Accepted range for parameters read back off disk -----------------
    //
    // The header is attacker-writable: it sits outside the AES-GCM ciphertext,
    // so nothing authenticates it before it is used. Feeding it straight to
    // Argon2 means `mCost = 4_000_000` asks the allocator for 4 GiB on unlock.
    // That is a local denial of service, not a path to the key, so the fix is a
    // ceiling — not a format change.
    //
    // The bounds are anchored to the only values this app has ever written
    // (`M_COST` / `T_COST` / `P_COST` above), with room to raise them later
    // without invalidating vaults sealed today.

    /// 8 MiB. An eighth of what we write. Argon2's own floor is `8 * p_cost`
    /// KiB, which would accept a 32 KiB vault — cheap enough to grind offline
    /// if an attacker ever gets the keychain salt too. Since 64 MiB is the only
    /// value this app has written, anything under this floor is a downgraded
    /// header, not a legacy vault.
    pub const M_COST_MIN: u32 = 8_192;
    /// 256 MiB — four times today's default, so the default can be raised
    /// twice (64 → 128 → 256) before this needs revisiting. A 256 MiB
    /// allocation is survivable on any machine that can run this app; the
    /// 4 GiB the review demonstrated is not.
    pub const M_COST_MAX: u32 = 262_144;

    /// Argon2's own minimum. `t_cost` is a linear time multiplier and buys far
    /// less than memory does, so the floor is the algorithm's rather than a
    /// stricter one of ours; the memory floor above is what costs an attacker.
    pub const T_COST_MIN: u32 = 1;
    /// 16 passes. Already several seconds over `M_COST_MAX` on desktop
    /// hardware, and `t_cost` scales unlock time linearly with no cap of its
    /// own — past this a header stops being a stronger vault and becomes a
    /// hang the user reads as a crash.
    pub const T_COST_MAX: u32 = 16;

    /// One lane, Argon2's minimum and what we write.
    pub const P_COST_MIN: u32 = 1;
    /// 16 lanes. More lanes than a desktop has cores buys nothing, and each
    /// lane is another slice of the memory budget to schedule; the `argon2`
    /// crate itself would accept up to 2^24, which is a second DoS knob.
    pub const P_COST_MAX: u32 = 16;

    /// Reject header parameters outside the documented range before Argon2
    /// gets to allocate anything.
    ///
    /// The error deliberately names the header, not the password: a tampered or
    /// corrupt file is a different problem from a typo, and telling the user
    /// "wrong password" for a 4 GiB `mCost` sends them to reset a vault that
    /// was fine.
    pub fn validate(&self) -> Result<(), String> {
        let out_of_range = !(Self::M_COST_MIN..=Self::M_COST_MAX).contains(&self.m_cost)
            || !(Self::T_COST_MIN..=Self::T_COST_MAX).contains(&self.t_cost)
            || !(Self::P_COST_MIN..=Self::P_COST_MAX).contains(&self.p_cost);
        if out_of_range {
            return Err(Self::OUT_OF_RANGE.to_string());
        }
        Ok(())
    }

    /// The one message for an out-of-range header, so callers and tests agree
    /// on it and it stays distinguishable from `vaultWrongPassword`.
    pub const OUT_OF_RANGE: &str = "vaultHeaderOutOfRange";
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: Self::M_COST,
            t_cost: Self::T_COST,
            p_cost: Self::P_COST,
        }
    }
}

/// On-disk envelope (`vault.edge`). The plaintext is a JSON array of
/// `ExchangeCredential`; only the ciphertext is ever written.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultFile {
    pub version: u32,
    pub kdf: KdfParams,
    /// Base64 AES-GCM nonce (12 bytes).
    pub nonce: String,
    /// Base64 AES-256-GCM ciphertext (includes the auth tag).
    pub ciphertext: String,
}

impl VaultFile {
    pub const VERSION: u32 = 1;
}

/// Lifecycle state reported to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VaultState {
    /// No vault file on disk yet.
    Absent,
    /// File exists but no key is held in memory.
    Locked,
    /// Key held; credentials available in memory.
    Unlocked,
}

/// Snapshot of vault state for the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub state: VaultState,
    pub credential_count: usize,
    /// Minutes of inactivity after which the vault locks itself.
    ///
    /// Reported rather than hardcoded in the UI so the panel states the budget
    /// the backend is actually enforcing. It is configurable
    /// (`ALEPH_EDGE_VAULT_IDLE_MINUTES`), and a translated string claiming
    /// "30 min" on a desk configured for 5 would be a lie about a security
    /// control.
    pub idle_timeout_minutes: u64,
}

/// Hand-written so a stray `{:?}` can never print a key: the secret, the key
/// and the passphrase are redacted. (`Serialize` stays, for the encrypted
/// vault file only.)
impl std::fmt::Debug for ExchangeCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExchangeCredential")
            .field("exchange_id", &self.exchange_id)
            .field("label", &self.label)
            .field("api_key", &"<redacted>")
            .field("api_secret", &"<redacted>")
            .field("passphrase", &self.passphrase.as_ref().map(|_| "<redacted>"))
            .field("permission", &self.permission)
            .field("added_at", &self.added_at)
            .finish()
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;

    #[test]
    fn debug_output_never_contains_the_secret() {
        let c = ExchangeCredential {
            exchange_id: "binance".into(),
            label: "main".into(),
            api_key: "KEY-abc123".into(),
            api_secret: "SECRET-xyz789".into(),
            passphrase: Some("PASS-1".into()),
            permission: CredentialPermission::TradeOnly,
            added_at: 1,
            updated_at: None,
        };
        let out = format!("{c:?}");
        for secret in ["KEY-abc123", "SECRET-xyz789", "PASS-1"] {
            assert!(!out.contains(secret), "{out}");
        }
    }
}
