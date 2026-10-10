//! One desk per data directory.
//!
//! Two processes on one edge.db each run the engine. Paper bots resume on
//! launch, so both trade the same bots: every fill and trade is written twice,
//! the daily stop counts each loss twice, and a Stop or Delete in one window is
//! undone by the other's next save (audit 2026-10-08, persistence 1).
//!
//! `tauri-plugin-single-instance`, registered first in `run`, hands a second
//! launch to the running window and exits. It keys on the app identifier, so
//! the release and TESTNET builds still run side by side, each on its own data
//! dir. This lock is the second guard, for the gap the plugin leaves: it only
//! hands over once the first process has created its message window, so two
//! launches a moment apart can both pass it. The lock is an exclusive OS lock
//! on `<app data>/edge.lock`, taken before any restore runs and held for the
//! life of the process. The OS drops it when the process dies, so a crash
//! leaves nothing stale behind.

use std::fs::File;
use std::path::Path;
use std::time::{Duration, Instant};

pub const LOCK_FILE: &str = "edge.lock";

/// How long a launch waits for the lock before giving up. A restart into a
/// new version starts the new process before the old one has exited.
pub const HANDOFF_WAIT: Duration = Duration::from_secs(3);

/// Held for the life of the process; dropping it releases the lock.
pub struct InstanceLock {
    _file: File,
}

/// Takes the lock on `dir/edge.lock`, retrying for up to `wait`.
///
/// `Err("alreadyRunning")` when another process holds it. Any other failure
/// (the dir cannot be created or written) is `Err("instanceLockFailed")`:
/// the caller decides whether to go on without the second guard.
pub fn acquire(dir: &Path, wait: Duration) -> Result<InstanceLock, String> {
    std::fs::create_dir_all(dir).map_err(|_| "instanceLockFailed".to_string())?;
    let path = dir.join(LOCK_FILE);
    let deadline = Instant::now() + wait;
    loop {
        match try_lock(&path) {
            Ok(file) => return Ok(InstanceLock { _file: file }),
            Err(Held::ByOther) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(Held::ByOther) => return Err("alreadyRunning".to_string()),
            Err(Held::Failed) => return Err("instanceLockFailed".to_string()),
        }
    }
}

enum Held {
    ByOther,
    Failed,
}

/// Windows: a handle opened with share mode 0 is exclusive; any other open,
/// from this process or another, fails with ERROR_SHARING_VIOLATION.
#[cfg(windows)]
fn try_lock(path: &Path) -> Result<File, Held> {
    use std::os::windows::fs::OpenOptionsExt;
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(path)
        .map_err(|e| match e.raw_os_error() {
            Some(ERROR_SHARING_VIOLATION) | Some(ERROR_LOCK_VIOLATION) => Held::ByOther,
            _ => Held::Failed,
        })
}

/// Unix: an exclusive, non-blocking flock on the open file.
#[cfg(unix)]
fn try_lock(path: &Path) -> Result<File, Held> {
    use std::os::unix::io::AsRawFd;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| Held::Failed)?;
    // SAFETY: flock on a descriptor this function owns; no memory is shared.
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc == 0 {
        return Ok(file);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::EWOULDBLOCK) => Err(Held::ByOther),
        _ => Err(Held::Failed),
    }
}

#[cfg(not(any(windows, unix)))]
fn try_lock(path: &Path) -> Result<File, Held> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| Held::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("aleph-instance-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn a_second_desk_on_the_same_dir_is_refused_until_the_first_exits() {
        let dir = temp_dir();
        let first = acquire(&dir, Duration::ZERO).expect("first launch takes the lock");
        assert_eq!(
            acquire(&dir, Duration::ZERO).err().as_deref(),
            Some("alreadyRunning"),
            "a second launch must not get a second engine on edge.db"
        );
        drop(first);
        let again = acquire(&dir, Duration::ZERO);
        assert!(again.is_ok(), "the lock is free once the first process lets go");
        drop(again);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn release_and_testnet_dirs_lock_independently() {
        let (a, b) = (temp_dir(), temp_dir());
        let release = acquire(&a, Duration::ZERO).expect("release");
        let testnet = acquire(&b, Duration::ZERO).expect("testnet runs beside release");
        drop((release, testnet));
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn a_launch_waits_out_a_handoff() {
        // A restart starts the new process while the old one is still exiting.
        let dir = temp_dir();
        let old = acquire(&dir, Duration::ZERO).expect("old process");
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(250));
            drop(old);
        });
        let new = acquire(&dir, Duration::from_secs(3));
        releaser.join().unwrap();
        assert!(new.is_ok(), "the new process gets the lock once the old one exits");
        drop(new);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
