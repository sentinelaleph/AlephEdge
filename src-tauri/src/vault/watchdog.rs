//! The periodic half of auto-lock: a background task that locks an idle vault
//! even when nothing is asking the vault for anything.
//!
//! # Why a timer and not only a check on access
//!
//! `VaultManager::use_session` already applies the expiry rule before serving
//! any credential, which is enough to guarantee that an expired session never
//! hands out a key. It is NOT enough to get the key out of memory, because with
//! no access there is no check — and "nobody touched this desk for six hours" is
//! precisely the case auto-lock exists for. A lazy-only design leaves the keys
//! decrypted in exactly the scenario it was written to defend against.
//!
//! # Why the engine keeps the vault alive, and why that is correct
//!
//! While a real position is open, the engine reads the credential every tick to
//! manage it (`bot::engine::live_manage`, `live_close`, `reconcile`). Each read
//! is a genuine use of the vault and refreshes the budget, so the vault stays
//! unlocked for as long as money is at risk. That is the intended trade-off, not
//! a bug: locking the vault out from under an open position would strand a
//! live stop-loss with no way to move it. Do not "fix" this. The vault locks
//! once the desk is genuinely idle — no open positions, no user.
//!
//! # Why this loop has no stop channel
//!
//! `bot::BotManager::ensure_loop` is the pattern this follows — a
//! `tauri::async_runtime::spawn`ed task with a bounded tick — with one
//! deliberate difference: there is no `watch` sender to stop it. The bot loop is
//! stoppable because bots start and stop; an auto-lock watchdog with an off
//! switch is an auto-lock that can be switched off, and the start/stop
//! interleavings are exactly how an unlocked vault ends up with no watchdog
//! running. So it is started once from the app's `setup` and runs for the life
//! of the process. The cost of that choice is one mutex acquisition and one
//! integer comparison per minute.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use super::VaultManager;

/// How often the idle budget is checked. Sets the worst-case lateness of a
/// lock: a vault that goes idle one tick after a check locks up to `TICK` late.
/// Sixty seconds against a 30-minute budget is a 3% overshoot — small enough not
/// to matter, long enough to cost nothing.
const TICK: Duration = Duration::from_secs(60);

/// Emitted once, on the tick that auto-locks the vault.
///
/// The UI reads vault state from a command it calls on mount, not a poll, so
/// without this event the panel would keep showing an unlocked vault and the
/// user's next action would fail with a bare "vault locked". The payload is the
/// same secret-free `VaultStatus` the commands return — nothing new crosses the
/// IPC boundary.
pub const AUTO_LOCKED_EVENT: &str = "vault:auto-locked";

/// Starts the watchdog. Called once, from the app's `setup`.
///
/// Unconditional on purpose: it runs whether or not a vault exists yet, because
/// a security control that has to be armed by some other code path is a control
/// that a later refactor forgets to arm. A locked or absent vault makes every
/// tick a no-op — `IdleTimer::expired` is false when there is nothing to expire.
pub fn spawn_idle_watchdog(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(TICK).await;

            // Scoped so the `State` borrow of `app` is released before the
            // await at the top of the next iteration.
            let locked = { app.state::<VaultManager>().lock_if_idle() };
            if !locked {
                continue;
            }

            let status = super::commands::vault_path(&app)
                .map(|path| app.state::<VaultManager>().status(&path));
            if let Ok(status) = status {
                // A failed emit is not worth retrying or logging: the window is
                // gone, which is the case where nothing is waiting for it. The
                // keys are already out of memory either way — the event reports
                // the lock, it does not perform it.
                let _ = app.emit(AUTO_LOCKED_EVENT, status);
            }
        }
    });
}
