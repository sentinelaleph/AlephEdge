//! Local backups of the user's setup and history, under
//! `<app_data_dir>/backups/<folder>/`.
//!
//! One is taken before every update install (`pre-<from>-to-<to>-<stamp>`),
//! and on demand from Settings (`manual-<stamp>`). A backup holds:
//! - `edge.db`: trade history, bot settings, DCA/Grid bots and their cycles,
//!   copied with SQLite's `VACUUM INTO` through the store's own connection
//!   (a consistent snapshot, never a raw copy of a database in use);
//! - `config.json`: risk level, simulated balance, daily stop;
//! - `vault.edge`: the exchange-key vault, copied as is (it is encrypted at
//!   rest; its salt stays in the OS keychain);
//! - `desk-id`: the identity paired phones and the Sentinel relay know this
//!   desk by (a restored backup without it would pair as a new desk).
//!
//! The newest `KEEP` folders stay; older ones are deleted after each new
//! backup. A backup is written to a `.partial` folder and renamed into place
//! only when complete, so a failed one never lists as a backup.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::store::StoreManager;

/// Folder under the app-data dir that holds the backups.
pub const BACKUP_DIR: &str = "backups";
/// Backup folders kept; older ones are deleted.
pub const KEEP: usize = 5;
/// The store file (see `StoreManager::with_conn`).
pub const DB_FILE: &str = "edge.db";
/// Plain files copied as they are, when present.
pub const PLAIN_FILES: [&str; 3] = ["config.json", "vault.edge", "desk-id"];

const PARTIAL: &str = ".partial";

/// Why a backup was taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// Before installing `to` over `from`.
    Update { from: String, to: String },
    /// "Back up now" in Settings.
    Manual,
}

/// One backup folder as Settings lists it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    pub name: String,
    /// "update" or "manual".
    pub kind: &'static str,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
    /// When it was taken (UTC, from the folder name), UNIX ms.
    pub at_ms: u64,
    pub size_bytes: u64,
    /// The files it holds.
    pub files: Vec<String>,
}

// ------------------------------------------------------------ pure helpers

/// `yyyymmddHHMMSS`, UTC.
pub fn utc_stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}{m:02}{d:02}{:02}{:02}{:02}", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Inverse of `utc_stamp`; None for anything that is not 14 digits of a
/// real date and time.
pub fn parse_stamp(s: &str) -> Option<u64> {
    if s.len() != 14 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |a: usize, b: usize| s[a..b].parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(4, 6)?, n(6, 8)?, n(8, 10)?, n(10, 12)?, n(12, 14)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    let days = days_from_civil(y, mo, d);
    if civil_from_days(days) != (y, mo, d) {
        return None;
    }
    let secs = days * 86_400 + h * 3600 + mi * 60 + se;
    u64::try_from(secs).ok().map(|s| s * 1000)
}

/// Days since 1970-01-01 -> (year, month, day), proleptic Gregorian.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A version as it may appear in a folder name: what a semver carries,
/// anything else dropped (the feed's version is not ours to trust as a path).
fn clean_version(v: &str) -> String {
    let s: String = v.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+')).take(32).collect();
    if s.is_empty() {
        "unknown".into()
    } else {
        s
    }
}

/// The folder name for a backup taken at `ms`.
pub fn folder_name(reason: &Reason, ms: u64) -> String {
    let stamp = utc_stamp(ms);
    match reason {
        Reason::Update { from, to } => format!("pre-{}-to-{}-{stamp}", clean_version(from), clean_version(to)),
        Reason::Manual => format!("manual-{stamp}"),
    }
}

/// (kind, from, to, at_ms) of a backup folder name; None for anything else
/// (a `.partial` folder, a file the user put there).
pub fn parse_folder(name: &str) -> Option<(&'static str, Option<String>, Option<String>, u64)> {
    if let Some(stamp) = name.strip_prefix("manual-") {
        return Some(("manual", None, None, parse_stamp(stamp)?));
    }
    let rest = name.strip_prefix("pre-")?;
    let (versions, stamp) = rest.rsplit_once('-')?;
    let at = parse_stamp(stamp)?;
    let (from, to) = versions.split_once("-to-")?;
    if from.is_empty() || to.is_empty() {
        return None;
    }
    Some(("update", Some(from.to_string()), Some(to.to_string()), at))
}

fn dir_size_and_files(dir: &Path) -> (u64, Vec<String>) {
    let mut size = 0;
    let mut files = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if let Ok(md) = e.metadata() {
                if md.is_file() {
                    size += md.len();
                    files.push(e.file_name().to_string_lossy().into_owned());
                }
            }
        }
    }
    files.sort();
    (size, files)
}

/// Backups under `root`, newest first. A missing root is an empty list.
pub fn list(root: &Path) -> Vec<BackupEntry> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<BackupEntry> = rd
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let (kind, from_version, to_version, at_ms) = parse_folder(&name)?;
            let (size_bytes, files) = dir_size_and_files(&e.path());
            Some(BackupEntry { name, kind, from_version, to_version, at_ms, size_bytes, files })
        })
        .collect();
    out.sort_by(|a, b| b.at_ms.cmp(&a.at_ms).then(b.name.cmp(&a.name)));
    out
}

/// Deletes all but the newest `keep` backups, and any `.partial` folder a
/// failed backup left. Returns the names deleted.
pub fn prune(root: &Path, keep: usize) -> Vec<String> {
    let mut gone = Vec::new();
    for e in list(root).into_iter().skip(keep) {
        if std::fs::remove_dir_all(root.join(&e.name)).is_ok() {
            gone.push(e.name);
        }
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.ends_with(PARTIAL) && std::fs::remove_dir_all(e.path()).is_ok() {
                gone.push(name);
            }
        }
    }
    gone
}

/// Takes one backup of `data_dir` into `<data_dir>/backups/<name>`: the
/// database through `copy_db` (which must write a consistent copy to the
/// path it is given), then the plain files that exist. Then prunes to
/// `KEEP`. Any failure removes the partial folder and returns an
/// `backup*Failed` code; nothing is listed for it.
pub fn take(data_dir: &Path, name: &str, copy_db: impl FnOnce(&Path) -> Result<(), String>) -> Result<BackupEntry, String> {
    let root = data_dir.join(BACKUP_DIR);
    std::fs::create_dir_all(&root).map_err(|e| format!("backupDirFailed|{e}"))?;
    let final_dir = root.join(name);
    if final_dir.exists() {
        return Err("backupExists".into());
    }
    let tmp = root.join(format!("{name}{PARTIAL}"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir(&tmp).map_err(|e| format!("backupDirFailed|{e}"))?;
    let filled = fill(data_dir, &tmp, copy_db).and_then(|()| {
        std::fs::rename(&tmp, &final_dir).map_err(|e| format!("backupDirFailed|{e}"))
    });
    if let Err(e) = filled {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(e);
    }
    prune(&root, KEEP);
    let (kind, from_version, to_version, at_ms) = parse_folder(name).unwrap_or(("manual", None, None, 0));
    let (size_bytes, files) = dir_size_and_files(&final_dir);
    Ok(BackupEntry { name: name.to_string(), kind, from_version, to_version, at_ms, size_bytes, files })
}

fn fill(data_dir: &Path, into: &Path, copy_db: impl FnOnce(&Path) -> Result<(), String>) -> Result<(), String> {
    let db = into.join(DB_FILE);
    copy_db(&db)?;
    restrict(&db)?;
    for f in PLAIN_FILES {
        let src = data_dir.join(f);
        if !src.is_file() {
            continue;
        }
        let dst = into.join(f);
        std::fs::copy(&src, &dst).map_err(|e| format!("backupCopyFailed|{f}: {e}"))?;
        restrict(&dst)?;
    }
    Ok(())
}

/// Backups hold the trade history and the (encrypted) vault: owner-only,
/// like the originals.
fn restrict(p: &Path) -> Result<(), String> {
    crate::vault::secure_file::restrict_to_owner(p).map_err(|e| format!("backupCopyFailed|{e}"))
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ------------------------------------------------------------ app surface

fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| format!("backupDirFailed|{e}"))
}

/// Takes a backup now through the store's connection. Used by the update
/// install (before any download) and by "Back up now".
pub fn take_now(app: &AppHandle, reason: Reason) -> Result<BackupEntry, String> {
    let dir = data_dir(app)?;
    let name = folder_name(&reason, now_ms());
    let store = app.state::<StoreManager>();
    take(&dir, &name, |dest| store.backup_into(&dir, dest))
}

#[tauri::command]
pub fn backup_list(app: AppHandle) -> Result<Vec<BackupEntry>, String> {
    Ok(list(&data_dir(&app)?.join(BACKUP_DIR)))
}

#[tauri::command]
pub fn backup_now(app: AppHandle) -> Result<BackupEntry, String> {
    take_now(&app, Reason::Manual)
}

/// Opens the backups folder in the file manager (created when missing).
#[tauri::command]
pub fn backup_open_dir(app: AppHandle) -> Result<(), String> {
    let dir = data_dir(&app)?.join(BACKUP_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| format!("backupDirFailed|{e}"))?;
    app.opener()
        .open_path(dir.display().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aleph-backup-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn stamps_are_utc_and_round_trip() {
        assert_eq!(utc_stamp(0), "19700101000000");
        // 2026-10-16 13:45:07 UTC
        let ms = 1_792_158_307_000;
        assert_eq!(utc_stamp(ms), "20261016134507");
        assert_eq!(parse_stamp("20261016134507"), Some(ms));
        // 2024-02-29 is a date, 2026-02-29 is not.
        assert!(parse_stamp("20240229000000").is_some());
        assert_eq!(parse_stamp("20260229000000"), None);
        assert_eq!(parse_stamp("20261016246000"), None);
        assert_eq!(parse_stamp("2026101613450"), None);
        assert_eq!(parse_stamp("2026101613450x"), None);
    }

    #[test]
    fn folder_names_carry_reason_versions_and_time() {
        let ms = 1_792_158_307_000;
        let up = Reason::Update { from: "0.3.14".into(), to: "0.3.15".into() };
        assert_eq!(folder_name(&up, ms), "pre-0.3.14-to-0.3.15-20261016134507");
        assert_eq!(folder_name(&Reason::Manual, ms), "manual-20261016134507");
        // A feed version is never a path.
        let odd = Reason::Update { from: "0.3.14".into(), to: "../../x".into() };
        assert_eq!(folder_name(&odd, ms), "pre-0.3.14-to-....x-20261016134507");
        assert_eq!(
            parse_folder("pre-0.3.14-to-0.3.15-20261016134507"),
            Some(("update", Some("0.3.14".into()), Some("0.3.15".into()), ms))
        );
        assert_eq!(parse_folder("manual-20261016134507"), Some(("manual", None, None, ms)));
        assert_eq!(parse_folder("manual-20261016134507.partial"), None);
        assert_eq!(parse_folder("pre--to-0.3.15-20261016134507"), None);
        assert_eq!(parse_folder("notes"), None);
    }

    #[test]
    fn prune_keeps_the_newest_five_and_clears_partials() {
        let root = temp_dir("prune");
        let base = 1_792_158_307_000u64;
        let mut names = Vec::new();
        for i in 0..7u64 {
            let reason = if i % 2 == 0 { Reason::Manual } else { Reason::Update { from: "0.3.14".into(), to: "0.3.15".into() } };
            let n = folder_name(&reason, base + i * 60_000);
            std::fs::create_dir_all(root.join(&n)).unwrap();
            std::fs::write(root.join(&n).join("edge.db"), b"x").unwrap();
            names.push(n);
        }
        std::fs::create_dir_all(root.join("manual-20261016000000.partial")).unwrap();
        std::fs::create_dir_all(root.join("keep-me")).unwrap();
        let gone = prune(&root, KEEP);
        assert!(gone.contains(&names[0]) && gone.contains(&names[1]), "{gone:?}");
        assert!(gone.contains(&"manual-20261016000000.partial".to_string()));
        let left: Vec<String> = list(&root).into_iter().map(|e| e.name).collect();
        let want: Vec<String> = names[2..].iter().rev().cloned().collect();
        assert_eq!(left, want);
        assert!(root.join("keep-me").is_dir(), "a folder that is not a backup is never deleted");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_backup_copies_the_live_database_and_the_plain_files() {
        let data = temp_dir("take");
        let store = StoreManager::new();
        // Opens edge.db in `data` (migrated) and writes rows through it.
        store.set_meta(&data, "bot_config.futures", "{\"kind\":\"futures\"}").unwrap();
        store.set_meta(&data, "running_kinds", "futures").unwrap();
        std::fs::write(data.join("config.json"), b"{\"level\":\"calm\"}").unwrap();
        std::fs::write(data.join("vault.edge"), b"sealed-bytes").unwrap();
        std::fs::write(data.join("desk-id"), b"desk-4f1c").unwrap();

        let name = folder_name(&Reason::Update { from: "0.3.14".into(), to: "0.3.15".into() }, 1_792_158_307_000);
        let entry = take(&data, &name, |dest| store.backup_into(&data, dest)).unwrap();
        assert_eq!(entry.kind, "update");
        assert_eq!(entry.files, vec!["config.json", "desk-id", "edge.db", "vault.edge"]);
        assert!(entry.size_bytes > 0);

        let dir = data.join(BACKUP_DIR).join(&name);
        let copy = rusqlite::Connection::open(dir.join(DB_FILE)).unwrap();
        let v: String = copy
            .query_row("SELECT value FROM meta WHERE key = 'running_kinds'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "futures");
        let n: i64 = copy.query_row("SELECT COUNT(*) FROM meta", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2);
        assert_eq!(std::fs::read(dir.join("vault.edge")).unwrap(), b"sealed-bytes");
        assert_eq!(std::fs::read(dir.join("desk-id")).unwrap(), b"desk-4f1c", "the pre-update backup keeps the desk identity");
        assert_eq!(list(&data.join(BACKUP_DIR)), vec![entry]);
        // The original keeps working after the copy.
        store.set_meta(&data, "after", "1").unwrap();
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn a_failed_backup_leaves_nothing_listed() {
        let data = temp_dir("fail");
        std::fs::write(data.join("config.json"), b"{}").unwrap();
        let err = take(&data, "manual-20261016134507", |_| Err("backupDbFailed|disk full".into())).unwrap_err();
        assert_eq!(err, "backupDbFailed|disk full");
        let root = data.join(BACKUP_DIR);
        assert!(list(&root).is_empty());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 0, "the partial folder is removed");
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn missing_plain_files_are_skipped() {
        let data = temp_dir("fresh");
        let store = StoreManager::new();
        let entry = take(&data, "manual-20261016134507", |dest| store.backup_into(&data, dest)).unwrap();
        assert_eq!(entry.files, vec!["edge.db"]);
        assert_eq!(take(&data, "manual-20261016134507", |_| Ok(())).unwrap_err(), "backupExists");
        let _ = std::fs::remove_dir_all(&data);
    }
}
