//! OS keychain entries, one set per app identifier.
//!
//! Builds before 0.2.1 kept the vault salt and the sign-in session under one
//! fixed service name, so the release, TESTNET and e2e builds shared both
//! entries: creating or resetting a vault in one build replaced or deleted the
//! salt every other build's vault needs (that vault then answered its correct
//! password with "Wrong password"), and signing in or out did the same to the
//! others' session. Each build keeps its entries under its own identifier. The
//! release identifier IS the old service name, so release users read exactly
//! the entries they always had.

use std::sync::OnceLock;

/// The one service name every build used before 0.2.1. Also the release
/// build's identifier (tauri.conf.json).
pub const LEGACY_SERVICE: &str = "com.sentinelaleph.edge";

static SERVICE: OnceLock<String> = OnceLock::new();

/// Sets this build's service name. Called once from `run`, before the app
/// builder exists, so no command can read a keychain entry before it.
pub fn init(identifier: &str) {
    let _ = SERVICE.set(identifier.to_string());
}

/// This build's keychain service: its app identifier.
pub fn service() -> &'static str {
    SERVICE.get().map(String::as_str).unwrap_or(LEGACY_SERVICE)
}

/// The keychain operations the vault and the session need. The OS keychain in
/// the app, a map in tests: no test ever touches the developer's real entries.
pub trait SecretStore {
    fn get(&self, service: &str, user: &str) -> Option<String>;
    /// `get` that tells a missing entry (`Ok(None)`, the keychain's own
    /// "no entry" answer) from a read that failed (`Err`). Only the first
    /// proves anything about the entry.
    fn lookup(&self, service: &str, user: &str) -> Result<Option<String>, ()> {
        Ok(self.get(service, user))
    }
    fn set(&self, service: &str, user: &str, value: &str) -> Result<(), ()>;
    fn delete(&self, service: &str, user: &str);
}

/// Windows Credential Manager / macOS Keychain (keyring's native stores).
pub struct OsKeychain;

impl SecretStore for OsKeychain {
    fn get(&self, service: &str, user: &str) -> Option<String> {
        keyring::Entry::new(service, user).ok()?.get_password().ok()
    }

    fn lookup(&self, service: &str, user: &str) -> Result<Option<String>, ()> {
        let entry = keyring::Entry::new(service, user).map_err(|_| ())?;
        no_entry_is_none(entry.get_password())
    }

    fn set(&self, service: &str, user: &str, value: &str) -> Result<(), ()> {
        keyring::Entry::new(service, user)
            .and_then(|e| e.set_password(value))
            .map_err(|_| ())
    }

    fn delete(&self, service: &str, user: &str) {
        if let Ok(entry) = keyring::Entry::new(service, user) {
            let _ = entry.delete_credential();
        }
    }
}

/// A keyring read: the value, `None` for the keychain's "no entry" answer,
/// `Err` for every other failure (locked store, platform error, ...).
fn no_entry_is_none(res: Result<String, keyring::Error>) -> Result<Option<String>, ()> {
    match res {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(()),
    }
}

#[cfg(test)]
pub mod memory {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::SecretStore;

    /// In-memory keychain for tests.
    #[derive(Default)]
    pub struct MemoryKeychain {
        entries: RefCell<HashMap<(String, String), String>>,
        /// Every read fails (a locked or unreachable store), as `lookup` sees it.
        pub failing: std::cell::Cell<bool>,
    }

    impl SecretStore for MemoryKeychain {
        fn get(&self, service: &str, user: &str) -> Option<String> {
            if self.failing.get() {
                return None;
            }
            self.entries.borrow().get(&(service.to_string(), user.to_string())).cloned()
        }

        fn lookup(&self, service: &str, user: &str) -> Result<Option<String>, ()> {
            if self.failing.get() {
                return Err(());
            }
            Ok(self.get(service, user))
        }

        fn set(&self, service: &str, user: &str, value: &str) -> Result<(), ()> {
            self.entries
                .borrow_mut()
                .insert((service.to_string(), user.to_string()), value.to_string());
            Ok(())
        }

        fn delete(&self, service: &str, user: &str) {
            self.entries.borrow_mut().remove(&(service.to_string(), user.to_string()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_no_entry_answer_reads_as_missing() {
        assert_eq!(no_entry_is_none(Ok("s".into())), Ok(Some("s".into())));
        assert_eq!(no_entry_is_none(Err(keyring::Error::NoEntry)), Ok(None));
        assert_eq!(no_entry_is_none(Err(keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("locked"))))), Err(()));
        assert_eq!(no_entry_is_none(Err(keyring::Error::PlatformFailure(Box::new(std::io::Error::other("rpc"))))), Err(()));
    }

    #[test]
    fn the_release_build_keeps_the_entry_name_it_always_had() {
        // tauri.conf.json's identifier; a rename here would cost every 0.2.0
        // user their vault salt.
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], LEGACY_SERVICE);
        let testnet: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.testnet.conf.json")).unwrap();
        assert_ne!(testnet["identifier"], LEGACY_SERVICE, "TESTNET keeps its own entries");
    }
}
