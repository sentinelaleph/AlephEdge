//! OS keychain entries, one set per app identifier.
//!
//! The vault salt and the sign-in session used one fixed service name, so the
//! release, TESTNET and e2e builds shared both entries. Creating or resetting a
//! vault in one build replaced or deleted the salt every other build's vault
//! needs, and that vault then answered its correct password with "Wrong
//! password". Signing in or out in one build did the same to the others'
//! session (audit 2026-10-08, persistence 2). Each build now keeps its entries
//! under its own identifier. The release identifier IS the old service name,
//! so release users read exactly the entries they always had.

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
    fn set(&self, service: &str, user: &str, value: &str) -> Result<(), ()>;
    fn delete(&self, service: &str, user: &str);
}

/// Windows Credential Manager / macOS Keychain (keyring's native stores).
pub struct OsKeychain;

impl SecretStore for OsKeychain {
    fn get(&self, service: &str, user: &str) -> Option<String> {
        keyring::Entry::new(service, user).ok()?.get_password().ok()
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

#[cfg(test)]
pub mod memory {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::SecretStore;

    /// In-memory keychain for tests.
    #[derive(Default)]
    pub struct MemoryKeychain {
        entries: RefCell<HashMap<(String, String), String>>,
    }

    impl SecretStore for MemoryKeychain {
        fn get(&self, service: &str, user: &str) -> Option<String> {
            self.entries.borrow().get(&(service.to_string(), user.to_string())).cloned()
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
