//! Global risk manager: owns the selected risk level, the simulated account
//! balance, and the daily-stop close preference; persists them to a small,
//! non-sensitive `config.json` in the app-data directory (settings are not
//! secrets; the vault is only for exchange keys).

mod commands;
pub mod model;

pub use commands::{
    risk_get, risk_set_balance, risk_set_close_on_stop, risk_set_daily_loss, risk_set_level,
};

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use model::{RiskConfig, RiskLevel, RiskLimits, RiskState};

pub struct RiskManager {
    config: Mutex<RiskConfig>,
}

impl RiskManager {
    /// Loads persisted config, defaulting to Cautious on first run or a
    /// malformed file (fail-safe: never default to a riskier level).
    pub fn load(app_data_dir: Option<PathBuf>) -> Self {
        let mut config = app_data_dir
            .map(|d| config_path(&d))
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|raw| serde_json::from_slice::<RiskConfig>(&raw).ok())
            .unwrap_or_default();
        // A hand-edited/corrupt balance ≤ 0 would silently disable BOTH the
        // per-position capital cap and the daily-loss kill-switch (both guard
        // `balance > 0`), so clamp on load too — `serde(default)` only covers a
        // missing field, not a present zero.
        config.balance = config.balance.max(1.0);
        Self {
            config: Mutex::new(config),
        }
    }

    /// The active hard limits every bot must respect.
    pub fn limits(&self) -> RiskLimits {
        self.config.lock().expect("risk mutex").level.limits()
    }

    /// Simulated account equity — the base for the percentage caps.
    pub fn balance(&self) -> f64 {
        self.config.lock().expect("risk mutex").balance
    }

    /// Whether the daily-loss stop should close open positions (PRD §5.3).
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

    /// Whether the current level permits the Pump bot (PRD §5.1).
    pub fn allows_pump(&self) -> bool {
        self.config.lock().expect("risk mutex").level.allows_pump()
    }

    pub fn state(&self) -> RiskState {
        RiskState::from_config(&self.config.lock().expect("risk mutex"))
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

    /// Applies a mutation, persists, and returns the fresh state. Persistence
    /// failure doesn't undo the in-memory change; it only means the choice
    /// won't survive a restart.
    fn update(&self, dir: &Path, mutate: impl FnOnce(&mut RiskConfig)) -> RiskState {
        let mut g = self.config.lock().expect("risk mutex");
        mutate(&mut g);
        let snapshot = g.clone();
        drop(g);
        let _ = std::fs::create_dir_all(dir);
        if let Ok(json) = serde_json::to_vec_pretty(&snapshot) {
            let _ = std::fs::write(config_path(dir), json);
        }
        RiskState::from_config(&snapshot)
    }
}

fn config_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}
