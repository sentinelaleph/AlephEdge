//! Owner-only, crash-safe file writes.
//!
//! This lives beside the vault because the vault is what forced it, but the
//! SQLite trade store and the persisted desk id use it too: every file this
//! app leaves in the app-data directory is either a secret, a record of what
//! the user traded, or an identity the phone pairs against, and none of the
//! three should land at whatever the umask happens to be.
//!
//! Two properties, both of which the old `fs::write` call lacked:
//!
//!   * **Owner-only.** A vault file at the umask default (0644 on most Unix
//!     systems) is world-readable. The ciphertext is still AES-256-GCM and the
//!     salt still lives in the OS keychain, so this is not a key-recovery hole
//!     — but it hands every local account the material to work on, and the
//!     trade store is plaintext.
//!   * **Atomic.** An in-place write that dies halfway — crash, power cut, full
//!     disk — leaves a truncated file, and for the vault that means every key
//!     the user has entered is gone with no recovery path (there is
//!     deliberately no password recovery, and a half-written vault is not
//!     something a password could open anyway). Writing to a sibling temp file
//!     and renaming means the target is only ever the old file or the new one.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Restrict an existing file to the owning user.
///
/// On Unix this is a literal 0600: owner read+write, nothing for group or
/// other.
///
/// On Windows this is deliberately a no-op. `fs::set_permissions` there can
/// only toggle the read-only attribute — it cannot express an ACL — so calling
/// it would mark the vault read-only (breaking the next save) while changing
/// nothing about who may open it. What Windows gives us instead is the default
/// ACL on `%APPDATA%\<app>`, which already grants only the owning user and
/// administrators; what it does not give us is a per-file guarantee we set
/// ourselves. Saying so is more useful to a reader than a `set_permissions`
/// call that looks portable and is not.
///
/// Neither platform protects the file from another process running as the same
/// user. That is the threat model of a local key vault and it is stated in the
/// README rather than papered over here.
pub fn restrict_to_owner(path: &Path) -> io::Result<()> {
    set_owner_only(path)
}

/// Owner read+write, nothing else.
#[cfg(unix)]
fn set_owner_only(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(OWNER_ONLY))
}

/// Windows: nothing to set. See `restrict_to_owner` for why this is a no-op
/// rather than a `set_permissions` call that would only mark the file
/// read-only.
#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// 0600. Named because it appears both here and on the temp file at creation,
/// and the two must not drift.
#[cfg(unix)]
const OWNER_ONLY: u32 = 0o600;

/// Write `bytes` to `path` so that an interrupted write leaves the previous
/// contents intact, and so the result is readable only by its owner.
///
/// Creates the parent directory if it is missing.
pub fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }

    // Same directory, because `rename` is only atomic within one filesystem;
    // a temp file in the system temp dir would silently degrade to copy+delete
    // across a mount boundary, which is exactly the non-atomic write we are
    // removing.
    let tmp = temp_sibling(path);

    if let Err(e) = fill_temp(&tmp, bytes) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    sync_parent_dir(path);
    Ok(())
}

/// Create the temp file, write the bytes, and get them onto the device.
///
/// The `sync_all` is not optional: a rename can reach stable storage ahead of
/// the data it renames, which would publish a correctly-named but empty vault —
/// the exact failure this module exists to prevent, moved one step later.
fn fill_temp(tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = create_private(tmp)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Create the temp file with the final permissions already on it.
///
/// The mode is set at creation, not afterwards: between creation and the
/// rename the temp file holds exactly the bytes the target will hold, so a
/// `set_permissions` call after the write would leave a window in which the
/// new vault is world-readable.
fn create_private(path: &Path) -> io::Result<File> {
    // A temp file left behind by an earlier crash keeps its old mode, because
    // `OpenOptions::mode` only applies to a file the call itself creates.
    // Remove it first so `create_new` below really does create.
    remove_if_present(path)?;

    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(OWNER_ONLY);
    }
    opts.open(path)
}

/// Delete `path` and any temp file an interrupted write left beside it.
///
/// `VaultManager::reset` is the only escape hatch when the password is lost, and
/// it is supposed to leave nothing behind. Deleting `vault.edge` alone would
/// leave a `vault.edge.tmp` holding the ciphertext of the very keys the user
/// just gave up on — reachable by anything that can read the app-data
/// directory, and outliving the keychain salt that reset also removes.
///
/// A missing file is not an error: reset has to be idempotent.
pub fn remove_with_temp(path: &Path) -> io::Result<()> {
    remove_if_present(&temp_sibling(path))?;
    remove_if_present(path)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn temp_sibling(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Best effort: fsync the directory so the rename itself survives a power cut.
///
/// Failure is ignored on purpose. The data is already durable at this point —
/// only the *name* is at risk — and there is no useful recovery for the caller,
/// who would otherwise be told a successful save failed. Windows has no
/// directory handle that can be synced this way, so this does nothing there.
#[cfg(unix)]
fn sync_parent_dir(path: &Path) {
    if let Some(dir) = path.parent() {
        let _ = File::open(dir).and_then(|handle| handle.sync_all());
    }
}

#[cfg(not(unix))]
fn sync_parent_dir(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let unique = uuid::Uuid::new_v4();
        let dir = std::env::temp_dir().join(format!("aleph-secure-file-{tag}-{unique}"));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_written_file_is_reachable_only_by_its_owner() {
        let dir = scratch("mode");
        let path = dir.join("vault.edge");
        write_private_atomic(&path, b"sealed").expect("write");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "the vault file must not be group- or world-readable"
            );
        }
        #[cfg(not(unix))]
        {
            // Skipped deliberately: Windows has no POSIX mode bits, and
            // `fs::set_permissions` there can only toggle read-only, so there
            // is no mode to assert. What we CAN assert is that we did not
            // "port" the fix by marking the file read-only — that would look
            // like hardening and would break the next save.
            eprintln!(
                "skipped: no POSIX mode on Windows; asserting the file stayed writable instead"
            );
            let perms = std::fs::metadata(&path).expect("metadata").permissions();
            assert!(
                !perms.readonly(),
                "a read-only vault file would break the next save"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_second_write_replaces_the_first() {
        // `rename` onto an existing path is the whole mechanism, and it is the
        // step most likely to differ between platforms — Windows needs
        // MOVEFILE_REPLACE_EXISTING semantics, which `fs::rename` provides. If
        // this ever regresses, adding a second credential stops working.
        let dir = scratch("replace");
        let path = dir.join("vault.edge");
        write_private_atomic(&path, b"one credential").expect("first write");
        write_private_atomic(&path, b"two credentials").expect("second write");

        assert_eq!(std::fs::read(&path).expect("read"), b"two credentials");
        assert!(
            !temp_sibling(&path).exists(),
            "the temp file must not linger"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_write_leaves_the_previous_contents_readable() {
        let dir = scratch("atomic");
        let path = dir.join("vault.edge");
        write_private_atomic(&path, b"first version").expect("first write");

        // Block the temp path with a directory: the write now fails before it
        // can touch the target. The old in-place `fs::write` truncated first
        // and asked questions later, which is the bug this proves gone.
        std::fs::create_dir(temp_sibling(&path)).expect("blocking dir");
        let err = write_private_atomic(&path, b"second version");
        assert!(err.is_err(), "the write was supposed to fail");

        assert_eq!(
            std::fs::read(&path).expect("previous file still readable"),
            b"first version",
            "a failed write must not damage the file that was already there"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_a_vault_takes_the_interrupted_copy_with_it() {
        // A reset that leaves `vault.edge.tmp` behind leaves the ciphertext of
        // the keys the user just abandoned sitting in the app-data directory.
        let dir = scratch("remove");
        let path = dir.join("vault.edge");
        std::fs::write(&path, b"sealed").expect("vault");
        std::fs::write(temp_sibling(&path), b"sealed too").expect("temp");

        remove_with_temp(&path).expect("remove");
        assert!(!path.exists());
        assert!(!temp_sibling(&path).exists());
        // Idempotent: reset must not fail because there was nothing to delete.
        remove_with_temp(&path).expect("second remove");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_temp_file_from_an_earlier_crash_does_not_block_the_next_write() {
        let dir = scratch("stale");
        let path = dir.join("vault.edge");
        std::fs::write(temp_sibling(&path), b"leftover").expect("stale temp");

        write_private_atomic(&path, b"fresh").expect("write over a stale temp");
        assert_eq!(std::fs::read(&path).expect("read"), b"fresh");
        assert!(
            !temp_sibling(&path).exists(),
            "the temp file must not survive"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
