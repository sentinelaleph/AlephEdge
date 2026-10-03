//! Local key vault: device-only, password-sealed exchange credentials.
//!
//! Absolute rule (PRD): vault contents never reach telemetry, logs, crash
//! reports, or ribqa.com. Only secret-free metadata (`CredentialMeta`,
//! `VaultStatus`) ever crosses the IPC boundary. Secrets live in the sealed
//! `vault.edge` file and, while unlocked, in a memory session. There is
//! deliberately no password recovery.
//!
//! Three storage properties, each of which was missing before publication and
//! each documented where it is implemented:
//!   * `vault.edge` is written owner-only and atomically (`secure_file`), so a
//!     crash cannot destroy the keys and a local account cannot read the file.
//!   * the KDF parameters in the header are bounded before Argon2 sees them
//!     (`model::KdfParams::validate`), because the header is outside the
//!     authenticated ciphertext and a tampered `mCost` is an allocation.
//!   * every credential wipes itself on drop (`model::ExchangeCredential`), not
//!     just the session's copy — the order path clones them constantly.
//!
//! And one lifetime property, added last and for the same reason (S4): the
//! unlocked session expires. An idle vault locks itself (`idle`, `watchdog`),
//! because a desk that runs unattended for days would otherwise hold decrypted
//! exchange keys in memory from the one password entry until the process died.

mod commands;
mod crypto;
mod idle;
pub mod model;
pub mod secure_file;
mod watchdog;

pub use commands::{
    vault_add_credential, vault_change_password, vault_create, vault_list_credentials,
    vault_lock, vault_remove_credential, vault_rename_credential, vault_replace_credential,
    vault_reset, vault_set_idle_minutes, vault_status, vault_unlock,
};
pub use commands::restore_idle_minutes;
pub use watchdog::spawn_idle_watchdog;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use zeroize::{Zeroize, Zeroizing};

use idle::IdleTimer;
use model::{CredentialMeta, ExchangeCredential, KdfParams, VaultFile, VaultState, VaultStatus};

/// The in-memory unlocked session.
///
/// Nothing wipes secrets here any more: the key is `Zeroizing`, and each
/// `ExchangeCredential` zeroizes in its own `Drop`, so dropping the `Vec` wipes
/// every element. A `Drop` on this struct that also walked `creds` would be
/// dead code — and worse, it would suggest the session is the only place a
/// credential gets wiped, which was exactly the gap: the copies handed out by
/// `credential()` never passed through here.
struct Session {
    key: Zeroizing<[u8; 32]>,
    params: KdfParams,
    creds: Vec<ExchangeCredential>,
}

/// Tauri-managed vault state. `None` session = locked or absent.
pub struct VaultManager {
    session: Mutex<Option<Session>>,
    /// How long the session may sit unused before it is dropped (`idle`).
    idle: IdleTimer,
}

impl VaultManager {
    pub fn new() -> Self {
        Self::with_idle(IdleTimer::from_config())
    }

    /// Same manager, explicit timer. The only seam the auto-lock tests need:
    /// they hand in a clock they drive by hand and a budget of milliseconds,
    /// instead of waiting out the configured half hour.
    fn with_idle(idle: IdleTimer) -> Self {
        Self {
            session: Mutex::new(None),
            idle,
        }
    }

    /// Takes the session lock AND records a use of the vault.
    ///
    /// Every operation that reads or writes credentials goes through here, which
    /// is the point: "idle" means the vault was not used, and there is exactly
    /// ONE place in this file that decides a call counts as a use. The
    /// alternative — an `idle.touch()` at the top of `add`, `list`, `remove`,
    /// `credential` and `unlock` — is a list a newly added accessor silently
    /// drops off, and the symptom of dropping off it is a vault that locks while
    /// it is being used.
    ///
    /// It applies the expiry rule first, so no caller is ever served credentials
    /// out of a session whose budget is already spent. The watchdog bounds how
    /// late the lock can be; this bounds how late it can be *and still hand out
    /// a key*.
    fn use_session(&self) -> MutexGuard<'_, Option<Session>> {
        let mut guard = self.session.lock().expect("vault mutex");
        self.expire_if_idle(&mut guard);
        self.idle.touch();
        guard
    }

    /// The single expiry rule, applied to an already-held guard.
    ///
    /// Assigns `None` directly instead of calling `self.lock()`, which would
    /// deadlock on the guard the caller is holding. Returns whether THIS call is
    /// what locked the vault, so the watchdog tells the UI once rather than
    /// every minute.
    fn expire_if_idle(&self, guard: &mut Option<Session>) -> bool {
        if guard.is_none() || !self.idle.expired() {
            return false;
        }
        // Dropping the session zeroizes: the key is `Zeroizing` and every
        // `ExchangeCredential` wipes itself in its own `Drop`. This changes WHEN
        // locking happens, never what locking does.
        *guard = None;
        self.idle.clear();
        true
    }

    /// Locks the vault if its idle budget is spent — the watchdog's whole job.
    ///
    /// True only on the tick that actually locked, never on the ones after it.
    pub fn lock_if_idle(&self) -> bool {
        let mut guard = self.session.lock().expect("vault mutex");
        self.expire_if_idle(&mut guard)
    }

    /// Report lifecycle state without touching secrets.
    ///
    /// Deliberately NOT `use_session`: the health strip polls this every four
    /// seconds, so counting a status read as a use would reset the idle budget
    /// forever and auto-lock would never fire for anybody with the window open.
    /// It does not expire either — a status report has no way to tell the UI it
    /// locked something, which is the watchdog's job, and the watchdog bounds
    /// the disagreement to one tick.
    pub fn status(&self, path: &Path) -> VaultStatus {
        let guard = self.session.lock().expect("vault mutex");
        match guard.as_ref() {
            Some(s) => VaultStatus {
                state: VaultState::Unlocked,
                credential_count: s.creds.len(),
                idle_timeout_minutes: self.idle.budget_minutes(),
            },
            None => VaultStatus {
                state: if path.exists() {
                    VaultState::Locked
                } else {
                    VaultState::Absent
                },
                credential_count: 0,
                idle_timeout_minutes: self.idle.budget_minutes(),
            },
        }
    }

    /// Create a new vault sealed under `password`. Fails if one already exists.
    pub fn create(&self, path: &Path, password: &str) -> Result<(), String> {
        if path.exists() {
            return Err("vaultExists".to_string());
        }
        let salt = crypto::random_bytes(crypto::SALT_LEN);
        save_salt(&B64.encode(&salt))?;
        let params = KdfParams::default();
        let key = crypto::derive_key(password.as_bytes(), &salt, &params)?;
        let creds: Vec<ExchangeCredential> = Vec::new();
        persist(path, &key, &params, &creds)?;
        *self.use_session() = Some(Session { key, params, creds });
        Ok(())
    }

    /// Unlock an existing vault. Wrong password fails as `vaultWrongPassword`.
    /// A missing keychain entry (vault file copied to another device/user
    /// account without its keychain-bound salt) fails with a distinct,
    /// honest error rather than a generic decrypt failure.
    pub fn unlock(&self, path: &Path, password: &str) -> Result<(), String> {
        let raw = std::fs::read(path).map_err(|_| "vaultFileUnreadable".to_string())?;
        let file: VaultFile =
            serde_json::from_slice(&raw).map_err(|_| "vaultFileCorrupt|file".to_string())?;
        let salt_b64 = load_salt()?;
        let salt = B64
            .decode(salt_b64.as_bytes())
            .map_err(|_| "vaultFileCorrupt|salt".to_string())?;
        let key = crypto::derive_key(password.as_bytes(), &salt, &file.kdf)?;
        let nonce = B64
            .decode(file.nonce.as_bytes())
            .map_err(|_| "vaultFileCorrupt|nonce".to_string())?;
        let ct = B64
            .decode(file.ciphertext.as_bytes())
            .map_err(|_| "vaultFileCorrupt|ciphertext".to_string())?;
        let mut plaintext = Zeroizing::new(crypto::open(&key, &nonce, &ct)?);
        let creds: Vec<ExchangeCredential> = serde_json::from_slice(&plaintext)
            .map_err(|_| "vaultFileCorrupt|contents".to_string())?;
        plaintext.zeroize();
        *self.use_session() = Some(Session {
            key,
            params: file.kdf,
            creds,
        });
        Ok(())
    }

    /// Drop the in-memory session (Drop zeroizes secrets).
    pub fn lock(&self) {
        *self.session.lock().expect("vault mutex") = None;
        // Clear, not touch: a locked vault has nothing left to expire, and a
        // stale stamp would have every watchdog tick believe it had work to do.
        self.idle.clear();
    }

    /// Destroys the vault entirely: locks the session, deletes `vault.edge`,
    /// and removes the keychain salt. The ONLY recovery path when the password
    /// is lost or the keychain salt is missing (PRD §5.5: no password
    /// recovery). After this the user creates a fresh vault and re-enters keys.
    /// Removes the temp file too: a save interrupted before its rename leaves
    /// `vault.edge.tmp` holding the same ciphertext, and a reset that deletes
    /// only `vault.edge` would leave that copy behind after the keychain salt
    /// is gone.
    pub fn reset(&self, path: &Path) -> Result<(), String> {
        self.lock();
        secure_file::remove_with_temp(path).map_err(|_| "vaultDeleteFailed".to_string())?;
        clear_salt();
        Ok(())
    }

    /// Add a credential and re-seal. Rejects a duplicate (exchange, label).
    pub fn add(&self, path: &Path, cred: ExchangeCredential) -> Result<(), String> {
        let mut guard = self.use_session();
        let s = guard.as_mut().ok_or("vaultLocked")?;
        if s.creds
            .iter()
            .any(|c| c.exchange_id == cred.exchange_id && c.label == cred.label)
        {
            return Err("credentialExists".to_string());
        }
        s.creds.push(cred);
        if let Err(e) = persist(path, &s.key, &s.params, &s.creds) {
            s.creds.pop();
            return Err(e);
        }
        Ok(())
    }

    /// Secret-free listing for the UI.
    pub fn list(&self) -> Result<Vec<CredentialMeta>, String> {
        let guard = self.use_session();
        let s = guard.as_ref().ok_or("vaultLocked")?;
        Ok(s.creds.iter().map(ExchangeCredential::to_meta).collect())
    }

    /// Returns a copy of the first stored credential for `exchange_id`, secrets
    /// included, for an authenticated exchange call (e.g. the account view).
    /// Only reachable while unlocked; the secret stays in-process, never logged
    /// or persisted anywhere but the sealed vault file.
    ///
    /// The clone is a real second copy of the secret on the heap. Hold it for
    /// the call and let it drop — `ExchangeCredential`'s `Drop` wipes it, so
    /// the cost of this convenience is bounded by the caller's scope. Do not
    /// move `api_key`/`api_secret` out of it; the borrow checker will refuse,
    /// which is the point.
    pub fn credential(&self, exchange_id: &str) -> Option<ExchangeCredential> {
        let guard = self.use_session();
        let s = guard.as_ref()?;
        s.creds
            .iter()
            .find(|c| c.exchange_id == exchange_id)
            .cloned()
    }

    /// Remove a credential by (exchange, label) and re-seal.
    pub fn remove(&self, path: &Path, exchange_id: &str, label: &str) -> Result<(), String> {
        let mut guard = self.use_session();
        let s = guard.as_mut().ok_or("vaultLocked")?;
        let Some(idx) = s
            .creds
            .iter()
            .position(|c| c.exchange_id == exchange_id && c.label == label)
        else {
            return Err("credentialNotFound".to_string());
        };
        // Same rollback as `add`: if the re-seal fails the key is still in
        // the file, so it must stay in the session too. Before, the UI showed
        // it removed, the next unlock brought it back, and the next
        // successful save of anything else silently completed the removal.
        let removed = s.creds.remove(idx);
        if let Err(e) = persist(path, &s.key, &s.params, &s.creds) {
            s.creds.insert(idx, removed);
            return Err(e);
        }
        Ok(())
    }

    /// Swap the key material of an existing (exchange, label) in place and
    /// re-seal. The old key stays in the file and the session until the new
    /// one is sealed, so a failed save never leaves the label without a key
    /// (remove-then-add did, whenever the second step was refused).
    pub fn replace(&self, path: &Path, new: ExchangeCredential) -> Result<(), String> {
        let mut guard = self.use_session();
        let s = guard.as_mut().ok_or("vaultLocked")?;
        let Some(idx) = s
            .creds
            .iter()
            .position(|c| c.exchange_id == new.exchange_id && c.label == new.label)
        else {
            return Err("credentialNotFound".to_string());
        };
        let old = std::mem::replace(&mut s.creds[idx], new);
        s.creds[idx].added_at = old.added_at;
        if let Err(e) = persist(path, &s.key, &s.params, &s.creds) {
            s.creds[idx] = old;
            return Err(e);
        }
        Ok(())
    }

    /// Rename a credential's label and re-seal. Bots pick a key by exchange,
    /// never by label, so a rename cannot orphan one.
    pub fn rename(&self, path: &Path, exchange_id: &str, from: &str, to: &str) -> Result<(), String> {
        let to = to.trim();
        if to.is_empty() || to.chars().count() > MAX_LABEL_CHARS {
            return Err(format!("credentialLabelInvalid|{MAX_LABEL_CHARS}"));
        }
        let mut guard = self.use_session();
        let s = guard.as_mut().ok_or("vaultLocked")?;
        if from != to && s.creds.iter().any(|c| c.exchange_id == exchange_id && c.label == to) {
            return Err("credentialExists".to_string());
        }
        let Some(idx) = s
            .creds
            .iter()
            .position(|c| c.exchange_id == exchange_id && c.label == from)
        else {
            return Err("credentialNotFound".to_string());
        };
        let old = std::mem::replace(&mut s.creds[idx].label, to.to_string());
        if let Err(e) = persist(path, &s.key, &s.params, &s.creds) {
            s.creds[idx].label = old;
            return Err(e);
        }
        Ok(())
    }

    /// Re-seal the vault under a new password. The current password must
    /// derive the session's key: an unlocked desk left open is not enough to
    /// take the vault over. The salt stays, because it binds the vault to this
    /// device's keychain and rewriting it would add a second write that can
    /// fail after the file already moved to the new key. The new KDF params
    /// are the current defaults, so an old vault picks up the current cost.
    pub fn change_password(&self, path: &Path, current: &str, new: &str) -> Result<(), String> {
        let salt = B64
            .decode(load_salt()?.as_bytes())
            .map_err(|_| "vaultFileCorrupt|salt".to_string())?;
        self.change_password_with_salt(path, current, new, &salt)
    }

    fn change_password_with_salt(
        &self,
        path: &Path,
        current: &str,
        new: &str,
        salt: &[u8],
    ) -> Result<(), String> {
        let mut guard = self.use_session();
        let s = guard.as_mut().ok_or("vaultLocked")?;
        let check = crypto::derive_key(current.as_bytes(), salt, &s.params)?;
        if !same_key(&check, &s.key) {
            return Err("vaultWrongPassword".to_string());
        }
        let params = KdfParams::default();
        let key = crypto::derive_key(new.as_bytes(), salt, &params)?;
        persist(path, &key, &params, &s.creds)?;
        s.key = key;
        s.params = params;
        Ok(())
    }

    /// Change the auto-lock budget for this process. The command persists it.
    pub fn set_idle_minutes(&self, minutes: u64) {
        self.idle.set_budget(std::time::Duration::from_secs(minutes * 60));
    }
}

/// Longest credential label (the add form's limit too).
pub const MAX_LABEL_CHARS: usize = 40;

/// Constant-time equality for two derived keys.
fn same_key(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Default for VaultManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Seal `creds` under `key` and write the vault file.
///
/// The write goes through `secure_file`: owner-only mode, and temp-file +
/// rename so an interrupted save cannot destroy the keys already in the vault.
/// There is no recovery path for a truncated vault — `reset()` is a delete, not
/// a repair — so the write itself has to be the safeguard.
fn persist(
    path: &Path,
    key: &[u8; 32],
    params: &KdfParams,
    creds: &[ExchangeCredential],
) -> Result<(), String> {
    let mut plaintext = Zeroizing::new(
        serde_json::to_vec(creds).map_err(|_| "vaultSerializeFailed".to_string())?,
    );
    let (nonce, ciphertext) = crypto::seal(key, &plaintext)?;
    plaintext.zeroize();
    let file = VaultFile {
        version: VaultFile::VERSION,
        kdf: params.clone(),
        nonce: B64.encode(nonce),
        ciphertext: B64.encode(ciphertext),
    };
    let json =
        serde_json::to_vec_pretty(&file).map_err(|_| "vaultSerializeFailed".to_string())?;
    secure_file::write_private_atomic(path, &json).map_err(|_| "vaultWriteFailed".to_string())
}

const KEYCHAIN_SERVICE: &str = "com.sentinelaleph.edge";
const KEYCHAIN_SALT_USER: &str = "vault-salt";

/// Writes the vault's salt to OS-native secure storage (Windows Credential
/// Manager / macOS Keychain / Linux Secret Service) — deliberately never to
/// `vault.edge` itself (PRD §5.5). This binds a vault to the device + OS user
/// account it was created on: the file alone, copied elsewhere, is not
/// decryptable even with the correct password.
fn save_salt(salt_b64: &str) -> Result<(), String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_SALT_USER)
        .and_then(|e| e.set_password(salt_b64))
        .map_err(|_| "keychainWriteFailed".to_string())
}

fn load_salt() -> Result<String, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_SALT_USER)
        .and_then(|e| e.get_password())
        .map_err(|_| "keychainSaltMissing".to_string())
}

/// Best-effort removal of the keychain salt (vault reset). A missing entry is
/// not an error.
fn clear_salt() {
    if let Ok(entry) = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_SALT_USER) {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use idle::TestClock;
    use model::CredentialPermission;
    use std::time::Duration;

    /// A manager whose idle budget is `budget_ms` on a clock the test drives.
    /// Returns the clock so the test can jump time instead of sleeping.
    fn manager(budget_ms: u64) -> (VaultManager, TestClock) {
        let clock = TestClock::default();
        let timer = IdleTimer::new(Duration::from_millis(budget_ms), Box::new(clock.clone()));
        (VaultManager::with_idle(timer), clock)
    }

    /// Installs an unlocked session without going through Argon2 or the OS
    /// keychain — these tests are about WHEN the session goes away, not how it
    /// was sealed.
    fn unlocked(m: &VaultManager, labels: &[&str]) {
        *m.use_session() = Some(Session {
            key: Zeroizing::new([3u8; 32]),
            params: KdfParams::default(),
            creds: labels.iter().map(|l| cred(l)).collect(),
        });
    }

    fn cred(label: &str) -> ExchangeCredential {
        ExchangeCredential {
            exchange_id: "binance".to_string(),
            label: label.to_string(),
            api_key: format!("key-{label}"),
            api_secret: format!("secret-{label}"),
            passphrase: None,
            permission: CredentialPermission::TradeOnly,
            added_at: 0,
            updated_at: None,
        }
    }

    /// Decrypt a vault file the way `unlock` does, minus the keychain.
    fn read_back(path: &Path, key: &[u8; 32]) -> Vec<ExchangeCredential> {
        let raw = std::fs::read(path).expect("vault file readable");
        let file: VaultFile = serde_json::from_slice(&raw).expect("vault file parses");
        let nonce = B64.decode(file.nonce.as_bytes()).expect("nonce");
        let ct = B64.decode(file.ciphertext.as_bytes()).expect("ciphertext");
        let plaintext = crypto::open(key, &nonce, &ct).expect("decrypt");
        serde_json::from_slice(&plaintext).expect("creds parse")
    }

    #[test]
    fn an_idle_vault_locks_itself_once_the_budget_is_spent() {
        // The gap S4 reported: before this, the keys stayed decrypted in memory
        // for the whole process lifetime after one password entry, on a desk
        // designed to run unattended for days.
        let (m, clock) = manager(1_000);
        unlocked(&m, &["main"]);
        assert!(m.list().is_ok(), "unlocked to begin with");

        clock.advance(1_000);
        assert!(m.lock_if_idle(), "the watchdog tick must lock it");
        assert_eq!(m.list().unwrap_err(), "vaultLocked");
    }

    #[test]
    fn a_use_inside_the_budget_defers_the_lock() {
        // "Idle" is not-used, not unlocked-a-while-ago. A desk managing an open
        // position reads the credential every tick and must stay unlocked.
        let (m, clock) = manager(1_000);
        unlocked(&m, &["main"]);

        for _ in 0..5 {
            clock.advance(900);
            assert!(!m.lock_if_idle(), "a used vault must not lock");
            // A credential read is a use — this is the engine's path.
            assert!(m.credential("binance").is_some());
        }

        clock.advance(1_000);
        assert!(m.lock_if_idle(), "left alone, it finally locks");
    }

    #[test]
    fn polling_the_status_is_not_a_use() {
        // The health strip calls `status` every four seconds. If that counted as
        // a use, the budget would never expire for anyone with the window open —
        // which is every user of a desk application.
        let (m, clock) = manager(1_000);
        unlocked(&m, &["main"]);
        let path = std::path::PathBuf::from("no-such-vault.edge");

        for _ in 0..10 {
            clock.advance(200);
            let status = m.status(&path);
            assert_eq!(status.state, VaultState::Unlocked);
        }
        assert!(m.lock_if_idle(), "status polling must not defer the lock");
    }

    #[test]
    fn an_expired_session_never_serves_a_credential() {
        // The watchdog bounds how late a lock can be; the access path bounds how
        // late it can be and still hand out a key. Without this, a credential
        // read in the gap between expiry and the next tick would succeed.
        let (m, clock) = manager(1_000);
        unlocked(&m, &["main"]);
        clock.advance(5_000);

        assert!(m.credential("binance").is_none(), "expired means expired");
        assert_eq!(m.list().unwrap_err(), "vaultLocked");
    }

    #[test]
    fn a_locked_vault_stays_locked_and_the_watchdog_stops_reporting() {
        // `lock_if_idle` must be true only on the tick that actually locked, or
        // the UI would be told the vault auto-locked once a minute forever.
        let (m, clock) = manager(1_000);
        unlocked(&m, &["main"]);
        clock.advance(1_000);
        assert!(m.lock_if_idle());

        for _ in 0..3 {
            clock.advance(10_000);
            assert!(!m.lock_if_idle(), "already locked; nothing to report");
            assert_eq!(m.list().unwrap_err(), "vaultLocked");
        }
    }

    #[test]
    fn a_manual_lock_leaves_nothing_for_the_watchdog_to_expire() {
        let (m, clock) = manager(1_000);
        unlocked(&m, &["main"]);
        m.lock();
        clock.advance(10_000);
        assert!(!m.lock_if_idle(), "a manual lock clears the stamp too");
    }

    #[test]
    fn the_budget_comes_from_configuration_and_is_reported_to_the_ui() {
        // The panel states the number the backend enforces. A hardcoded "30 min"
        // in a translation file would be a lie on any desk that configured it.
        let path = std::path::PathBuf::from("no-such-vault.edge");
        let configured = VaultManager::new();
        assert_eq!(
            configured.status(&path).idle_timeout_minutes,
            crate::app::endpoints::vault_idle_minutes(),
            "status must report the configured budget, not a constant"
        );

        let (five, _) = manager(5 * 60 * 1_000);
        assert_eq!(five.status(&path).idle_timeout_minutes, 5);
    }

    #[test]
    fn a_removal_that_cannot_be_saved_is_not_applied_in_memory() {
        let dir = std::env::temp_dir().join(format!("aleph-vault-{}", uuid::Uuid::new_v4()));
        let path = dir.join("vault.edge");
        let (m, _clock) = manager(60_000);
        unlocked(&m, &["main", "second"]);
        {
            let guard = m.use_session();
            let s = guard.as_ref().unwrap();
            persist(&path, &s.key, &s.params, &s.creds).expect("first seal");
        }
        // Block the temp path so the re-seal fails (see the test below).
        let mut tmp = path.clone().into_os_string();
        tmp.push(".tmp");
        std::fs::create_dir(std::path::PathBuf::from(tmp)).expect("blocking dir");

        assert_eq!(
            m.remove(&path, "binance", "second").unwrap_err(),
            "vaultWriteFailed"
        );
        let labels: Vec<String> = m.list().unwrap().into_iter().map(|c| c.label).collect();
        assert_eq!(
            labels,
            vec!["main".to_string(), "second".to_string()],
            "the session must still match the file"
        );
        assert_eq!(read_back(&path, &[3u8; 32]).len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_interrupted_save_leaves_the_previous_vault_unlockable() {
        // The old in-place `fs::write` truncated the target before it had the
        // new bytes: a crash mid-write took every key with it, and there is no
        // recovery path for a half-written vault — no password opens it.
        let dir = std::env::temp_dir().join(format!("aleph-vault-{}", uuid::Uuid::new_v4()));
        let path = dir.join("vault.edge");
        let key = [7u8; 32];
        let params = KdfParams::default();

        persist(&path, &key, &params, &[cred("main")]).expect("first seal");

        // Block the temp path with a directory so the save fails before the
        // rename — the same observable outcome as a crash or a full disk.
        // The ".tmp" suffix mirrors `secure_file::temp_sibling`, which is
        // private to that module; if it ever changes, this test stops
        // simulating a failure and starts passing for the wrong reason, so the
        // assertion on the error below is what keeps it honest.
        let mut tmp = path.clone().into_os_string();
        tmp.push(".tmp");
        std::fs::create_dir(std::path::PathBuf::from(tmp)).expect("blocking dir");

        let second = persist(&path, &key, &params, &[cred("main"), cred("second")]);
        assert_eq!(second.unwrap_err(), "vaultWriteFailed");

        let survivors = read_back(&path, &key);
        assert_eq!(survivors.len(), 1, "the first vault must still be intact");
        assert_eq!(survivors[0].label, "main");

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sealed(tag: &str, m: &VaultManager) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("aleph-vault-{tag}-{}", uuid::Uuid::new_v4()));
        let path = dir.join("vault.edge");
        let guard = m.use_session();
        let s = guard.as_ref().unwrap();
        persist(&path, &s.key, &s.params, &s.creds).expect("first seal");
        (dir, path)
    }

    #[test]
    fn a_renewed_key_replaces_the_old_one_and_keeps_its_added_date() {
        let (m, _c) = manager(60_000);
        unlocked(&m, &["main"]);
        if let Some(s) = m.use_session().as_mut() {
            s.creds[0].added_at = 42;
        }
        let (dir, path) = sealed("renew", &m);
        let mut new = cred("main");
        new.api_key = "key-new".into();
        new.updated_at = Some(99);
        m.replace(&path, new).expect("renew");
        let back = read_back(&path, &[3u8; 32]);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].api_key, "key-new");
        assert_eq!((back[0].added_at, back[0].updated_at), (42, Some(99)));
        assert_eq!(m.replace(&path, cred("other")).unwrap_err(), "credentialNotFound");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_renewal_that_cannot_be_saved_keeps_the_old_key() {
        let (m, _c) = manager(60_000);
        unlocked(&m, &["main"]);
        let (dir, path) = sealed("renewfail", &m);
        let mut tmp = path.clone().into_os_string();
        tmp.push(".tmp");
        std::fs::create_dir(std::path::PathBuf::from(tmp)).expect("blocking dir");
        let mut new = cred("main");
        new.api_key = "key-new".into();
        assert_eq!(m.replace(&path, new).unwrap_err(), "vaultWriteFailed");
        assert_eq!(m.credential("binance").unwrap().api_key, "key-main");
        assert_eq!(read_back(&path, &[3u8; 32])[0].api_key, "key-main");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rename_refuses_duplicates_and_blank_labels() {
        let (m, _c) = manager(60_000);
        unlocked(&m, &["main", "second"]);
        let (dir, path) = sealed("rename", &m);
        assert_eq!(m.rename(&path, "binance", "main", "second").unwrap_err(), "credentialExists");
        assert!(m.rename(&path, "binance", "main", "  ").unwrap_err().starts_with("credentialLabelInvalid"));
        assert_eq!(m.rename(&path, "binance", "nope", "x").unwrap_err(), "credentialNotFound");
        m.rename(&path, "binance", "main", " primary ").expect("rename");
        let labels: Vec<String> = read_back(&path, &[3u8; 32]).iter().map(|c| c.label.clone()).collect();
        assert_eq!(labels, vec!["primary".to_string(), "second".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_password_change_needs_the_current_password_and_reseals_under_the_new_one() {
        let salt = [5u8; crypto::SALT_LEN];
        let params = KdfParams::default();
        let old_key = crypto::derive_key(b"old-password", &salt, &params).unwrap();
        let (m, _c) = manager(60_000);
        *m.use_session() = Some(Session { key: old_key, params, creds: vec![cred("main")] });
        let (dir, path) = sealed("pw", &m);

        assert_eq!(
            m.change_password_with_salt(&path, "wrong-password", "new-password", &salt).unwrap_err(),
            "vaultWrongPassword"
        );
        m.change_password_with_salt(&path, "old-password", "new-password", &salt).expect("change");
        let new_key = crypto::derive_key(b"new-password", &salt, &KdfParams::default()).unwrap();
        assert_eq!(read_back(&path, &new_key)[0].label, "main");
        // The session moved too: a second change must prove the NEW password.
        assert_eq!(
            m.change_password_with_salt(&path, "old-password", "x-password", &salt).unwrap_err(),
            "vaultWrongPassword"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_auto_lock_budget_can_be_changed_while_unlocked() {
        let (m, _c) = manager(60_000);
        m.set_idle_minutes(5);
        assert_eq!(m.status(Path::new("no-such.edge")).idle_timeout_minutes, 5);
    }
}
