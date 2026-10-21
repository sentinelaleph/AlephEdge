//! Global risk manager: owns the selected risk level, the simulated account
//! balance, and the daily-stop close preference; persists them to a small,
//! non-sensitive `config.json` in the app-data directory (settings are not
//! secrets; the vault is only for exchange keys).
//!
//! The file is written atomically and owner-only (`secure_file`): a crash
//! mid-write leaves the previous settings, never a truncated file. A file
//! that exists but cannot be read is never silently replaced by the
//! defaults: its bytes are kept as `config.json.unreadable.<ms>`, the state
//! says so (`RiskState::config_unreadable`), and no bot starts or resumes
//! until the user saves the risk settings again (`app::data_health`).

mod commands;
pub mod model;

pub use commands::{
    risk_get, risk_set_balance, risk_set_close_on_stop, risk_set_daily_loss, risk_set_level,
    risk_set_view_thresholds,
};

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use model::{RiskConfig, RiskLevel, RiskLimits, RiskState, ViewThresholds};

pub struct RiskManager {
    config: Mutex<RiskConfig>,
    /// The settings file could not be read at load: the name of the kept
    /// copy ("" when even that could not be written). Cleared by the next
    /// successful save, which the user makes on the risk page.
    unreadable: Mutex<Option<String>>,
}

/// What the stored settings file held at load (pure input of `load`).
#[derive(Debug)]
pub enum Stored {
    /// No file: a first run.
    Missing,
    Bytes(Vec<u8>),
    /// The file exists but reading it failed.
    ReadFailed,
}

/// The config `stored` yields, or None when it exists and cannot be used
/// (pure). Missing = defaults (Cautious, the fail-safe first-run level).
pub fn parse_stored(stored: &Stored) -> Option<RiskConfig> {
    match stored {
        Stored::Missing => Some(RiskConfig::default()),
        Stored::Bytes(raw) => serde_json::from_slice::<RiskConfig>(raw).ok(),
        Stored::ReadFailed => None,
    }
}

fn read_stored(path: &Path) -> Stored {
    match std::fs::read(path) {
        Ok(raw) => Stored::Bytes(raw),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Stored::Missing,
        Err(_) => Stored::ReadFailed,
    }
}

impl RiskManager {
    /// Loads persisted config, defaulting to Cautious on first run. A file
    /// that exists but does not parse (or cannot be read) is kept as a copy
    /// and flagged; the defaults run in memory only (fail-safe: never a
    /// riskier level), and bots stay stopped until the user saves.
    pub fn load(app_data_dir: Option<PathBuf>) -> Self {
        let (parsed, unreadable) = match app_data_dir.as_deref() {
            None => (Some(RiskConfig::default()), None),
            Some(dir) => {
                let stored = read_stored(&config_path(dir));
                match parse_stored(&stored) {
                    Some(c) => (Some(c), None),
                    None => (None, Some(keep_unreadable(dir, &stored))),
                }
            }
        };
        let mut config = parsed.unwrap_or_default();
        // A hand-edited/corrupt balance ≤ 0 would silently disable BOTH the
        // per-position capital cap and the daily-loss kill-switch (both guard
        // `balance > 0`), so clamp on load too — `serde(default)` only covers a
        // missing field, not a present zero.
        config.balance = config.balance.max(1.0);
        if unreadable.is_some() {
            eprintln!("risk settings unreadable: running on the defaults until the user saves");
        }
        Self {
            config: Mutex::new(config),
            unreadable: Mutex::new(unreadable),
        }
    }

    /// The kept copy's file name when the stored settings could not be read
    /// at load and have not been saved since.
    pub fn unreadable(&self) -> Option<String> {
        self.unreadable.lock().expect("risk mutex").clone()
    }

    /// The active hard limits every bot must respect.
    pub fn limits(&self) -> RiskLimits {
        self.config.lock().expect("risk mutex").level.limits()
    }

    /// Simulated account equity — the base for the percentage caps.
    pub fn balance(&self) -> f64 {
        self.config.lock().expect("risk mutex").balance
    }

    /// Whether the daily-loss stop should close open positions.
    pub fn close_on_stop(&self) -> bool {
        self.config.lock().expect("risk mutex").close_on_stop
    }

    /// The daily-loss stop actually in force = min(level cap, user tolerance).
    /// The engine must read THIS, never `limits().daily_loss_limit_pct`, or a
    /// user who asked for a 2% stop would silently get their level's 15%.
    pub fn effective_daily_loss_pct(&self) -> f64 {
        self.config
            .lock()
            .expect("risk mutex")
            .effective_daily_loss_pct()
    }

    /// Sets (or clears, with `None`) the user's own daily-loss tolerance.
    pub fn set_daily_loss_override(&self, dir: &Path, pct: Option<f64>) -> RiskState {
        self.update(dir, |c| c.daily_loss_override_pct = pct)
    }

    /// The portfolio risk view's alert thresholds.
    pub fn view_thresholds(&self) -> ViewThresholds {
        self.config.lock().expect("risk mutex").view_thresholds
    }

    /// Stores new view thresholds after their range check (Err = i18n code).
    pub fn set_view_thresholds(&self, dir: &Path, t: ViewThresholds) -> Result<RiskState, &'static str> {
        t.check()?;
        Ok(self.update(dir, |c| c.view_thresholds = t))
    }

    pub fn state(&self) -> RiskState {
        let mut s = RiskState::from_config(&self.config.lock().expect("risk mutex"));
        s.config_unreadable = self.unreadable();
        s
    }

    pub fn set_level(&self, dir: &Path, level: RiskLevel) -> RiskState {
        self.update(dir, |c| c.level = level)
    }

    /// Sets the simulated balance (clamped to a sane positive minimum).
    pub fn set_balance(&self, dir: &Path, balance: f64) -> RiskState {
        self.update(dir, |c| c.balance = balance.max(1.0))
    }

    pub fn set_close_on_stop(&self, dir: &Path, close: bool) -> RiskState {
        self.update(dir, |c| c.close_on_stop = close)
    }

    /// Applies a mutation, persists atomically, and returns the fresh state.
    /// Persistence failure doesn't undo the in-memory change; it only means
    /// the choice won't survive a restart (and an unreadable file stays
    /// flagged). A successful save is the user's answer to an unreadable
    /// file: the flag clears, the kept copy stays.
    fn update(&self, dir: &Path, mutate: impl FnOnce(&mut RiskConfig)) -> RiskState {
        let mut g = self.config.lock().expect("risk mutex");
        mutate(&mut g);
        let snapshot = g.clone();
        drop(g);
        let saved = serde_json::to_vec_pretty(&snapshot)
            .ok()
            .is_some_and(|json| crate::vault::secure_file::write_private_atomic(&config_path(dir), &json).is_ok());
        if saved {
            *self.unreadable.lock().expect("risk mutex") = None;
        }
        self.state()
    }
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Copies an unreadable settings file next to it before anything can write
/// over it; returns the copy's name ("" when there was nothing to copy or
/// the copy failed: the original is then still untouched on disk).
fn keep_unreadable(dir: &Path, stored: &Stored) -> String {
    let Stored::Bytes(raw) = stored else {
        return String::new();
    };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let name = format!("config.json.unreadable.{ms}");
    match crate::vault::secure_file::write_private_atomic(&dir.join(&name), raw) {
        Ok(()) => name,
        Err(_) => String::new(),
    }
}

#[cfg(test)]
mod load_tests {
    use super::*;
    use model::RiskLevel;

    fn scratch() -> PathBuf {
        let d = std::env::temp_dir().join(format!("ae-risk-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_first_run_is_cautious_and_not_flagged() {
        let d = scratch();
        let m = RiskManager::load(Some(d.clone()));
        assert_eq!(m.state().limits.level, RiskLevel::Cautious);
        assert_eq!(m.unreadable(), None);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_corrupt_file_is_kept_flagged_and_not_overwritten_by_the_defaults() {
        let d = scratch();
        std::fs::write(d.join("config.json"), b"{\"level\":\"greedy\",\"balance\":").unwrap();
        let m = RiskManager::load(Some(d.clone()));
        let kept = m.unreadable().expect("flagged");
        assert!(kept.starts_with("config.json.unreadable."), "{kept}");
        assert_eq!(std::fs::read(d.join(&kept)).unwrap(), b"{\"level\":\"greedy\",\"balance\":");
        // The original is untouched until the user saves.
        assert_eq!(std::fs::read(d.join("config.json")).unwrap(), b"{\"level\":\"greedy\",\"balance\":");
        assert_eq!(m.state().config_unreadable.as_deref(), Some(kept.as_str()));
        // Defaults run in memory (never a riskier level than the file's).
        assert_eq!(m.state().limits.level, RiskLevel::Cautious);
        // The user's save clears the flag and writes a whole, readable file.
        m.set_level(&d, RiskLevel::Balanced);
        assert_eq!(m.unreadable(), None);
        let back = RiskManager::load(Some(d.clone()));
        assert_eq!(back.state().limits.level, RiskLevel::Balanced);
        assert_eq!(back.unreadable(), None);
        assert!(d.join(&kept).exists(), "the kept copy stays");
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn an_unreadable_file_is_flagged_even_with_nothing_to_copy() {
        assert!(parse_stored(&Stored::ReadFailed).is_none());
        assert!(parse_stored(&Stored::Bytes(b"[]".to_vec())).is_none());
        assert!(parse_stored(&Stored::Missing).is_some());
        assert_eq!(keep_unreadable(Path::new("."), &Stored::ReadFailed), "");
    }

    #[test]
    fn a_save_leaves_no_temp_file_and_reads_back() {
        let d = scratch();
        let m = RiskManager::load(Some(d.clone()));
        m.set_balance(&d, 2_500.0);
        let names: Vec<String> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, vec!["config.json".to_string()], "atomic write, no leftovers");
        assert_eq!(RiskManager::load(Some(d.clone())).balance(), 2_500.0);
        let _ = std::fs::remove_dir_all(d);
    }
}
