//! Session persistence: the refresh token + user identity in the OS keychain,
//! so the next app launch can restore a signed-in session without a password.
//! Isolated from the manager so keychain I/O (blocking OS calls) stays out of
//! the state-machine file.
//!
//! One entry per app identifier (`app::keychain`): signing in or out in the
//! TESTNET build no longer replaces or clears the release build's session.
//! There is deliberately no copy from the pre-0.2.1 shared entry. Refresh
//! tokens rotate on every refresh, so two builds holding one token would sign
//! each other out; a non-release build signs in once instead. The release
//! build's entry name is unchanged, so its users stay signed in.

use super::model::PersistedSession;

#[cfg_attr(test, allow(dead_code))]
const KEYCHAIN_SESSION_USER: &str = "membership-session";

/// Persists the refresh token + user identity to the OS keychain. Best-effort:
/// a write failure here just means the next launch asks for a password again
/// (same as before this existed), so it never blocks the caller's own result.
pub(super) fn persist_session(session: &PersistedSession) {
    let Ok(json) = serde_json::to_string(session) else {
        return;
    };
    store::set(&json);
}

pub(super) fn load_persisted_session() -> Option<PersistedSession> {
    serde_json::from_str(&store::get()?).ok()
}

pub(super) fn clear_persisted_session() {
    store::clear();
}

/// The OS keychain entry, under this build's own service.
#[cfg(not(test))]
mod store {
    use super::KEYCHAIN_SESSION_USER;
    use crate::app::keychain::{service, OsKeychain, SecretStore};

    pub fn set(json: &str) {
        let _ = OsKeychain.set(service(), KEYCHAIN_SESSION_USER, json);
    }

    pub fn get() -> Option<String> {
        OsKeychain.get(service(), KEYCHAIN_SESSION_USER)
    }

    pub fn clear() {
        OsKeychain.delete(service(), KEYCHAIN_SESSION_USER);
    }
}

/// Tests never touch the real keychain: the entry name is the production
/// one, so a test that signs in or out would overwrite the developer's own
/// saved session. Per-thread, so parallel tests do not see each other.
#[cfg(test)]
pub(super) mod store {
    use std::cell::RefCell;

    thread_local! {
        static ENTRY: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub fn set(json: &str) {
        ENTRY.with(|e| *e.borrow_mut() = Some(json.to_string()));
    }

    pub fn get() -> Option<String> {
        ENTRY.with(|e| e.borrow().clone())
    }

    pub fn clear() {
        ENTRY.with(|e| *e.borrow_mut() = None);
    }
}
