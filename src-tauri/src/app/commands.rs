//! Top-level Tauri commands that compose subsystem state (vault, membership,
//! exchange, signal) into the health snapshot. Each subsystem owns its own
//! command module; this file holds only the app-level surface so `lib.rs`
//! stays a thin composition root.

use tauri::{AppHandle, Manager, State};

use crate::app::health::{
    ExchangeHealth, HealthLevel, HealthSnapshot, MembershipState, SignalHealth as HealthSignal,
    VaultState as HealthVaultState,
};
use crate::exchange::model::{ExchangeInfo, ExchangeStatus};
use crate::exchange::ExchangeManager;
use crate::market::MarketManager;
use crate::membership::model::MembershipHealth;
use crate::membership::MembershipManager;
use crate::signal::model::SignalHealthInfo;
use crate::signal::SignalManager;
use crate::vault::model::VaultState;
use crate::vault::VaultManager;

/// Returns a health snapshot: vault, membership, every catalog exchange, the
/// signal feed and the BTC pulse, each read live.
#[tauri::command]
pub async fn get_health_snapshot(
    app: AppHandle,
    vault: State<'_, VaultManager>,
    membership: State<'_, MembershipManager>,
    exchange: State<'_, ExchangeManager>,
    signal: State<'_, SignalManager>,
    market: State<'_, MarketManager>,
) -> Result<HealthSnapshot, String> {
    let mut snapshot = HealthSnapshot::unknown();

    if let Ok(dir) = app.path().app_data_dir() {
        snapshot.vault = map_vault(vault.status(&dir.join("vault.edge")).state);
    }
    snapshot.membership = map_membership(membership.health_state());

    // Every catalog exchange, live and concurrent.
    snapshot.exchanges = exchange
        .all_status("BTCUSDT")
        .await
        .into_iter()
        .map(|(info, status)| exchange_row(&info, &status))
        .collect();
    snapshot.signal = map_signal(signal.health());

    // BTC macro pulse (Sentinel). On a feed error the BTC pulse stays empty;
    // nothing is synthesized.
    if let Ok(btc) = market.btc(&membership.base_url()).await {
        snapshot.btc.trend = btc.trend;
        snapshot.btc.strength = btc.trend_strength;
        snapshot.btc.phase = btc.market_phase;
        snapshot.btc.price = btc.price;
        snapshot.btc.change_pct = btc.change_pct;
    }

    Ok(snapshot)
}

/// App/build info surfaced in the UI footer.
#[tauri::command]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn map_vault(state: VaultState) -> HealthVaultState {
    match state {
        VaultState::Absent => HealthVaultState::Absent,
        VaultState::Locked => HealthVaultState::Locked,
        VaultState::Unlocked => HealthVaultState::Unlocked,
    }
}

fn map_membership(state: MembershipHealth) -> MembershipState {
    match state {
        MembershipHealth::Active => MembershipState::Active,
        MembershipHealth::Expiring => MembershipState::Expiring,
        MembershipHealth::Inactive => MembershipState::Inactive,
        MembershipHealth::Unknown => MembershipState::Unknown,
    }
}

fn exchange_row(info: &ExchangeInfo, status: &ExchangeStatus) -> ExchangeHealth {
    ExchangeHealth {
        id: info.id.clone(),
        name: info.name.clone(),
        level: if status.healthy {
            HealthLevel::Ok
        } else {
            HealthLevel::Down
        },
        latency_ms: status.latency_ms,
    }
}

fn map_signal(info: SignalHealthInfo) -> HealthSignal {
    HealthSignal {
        level: if info.connected {
            HealthLevel::Ok
        } else {
            HealthLevel::Down
        },
        connected: info.connected,
        last_signal_secs: info.last_signal_secs,
        latency_ms: info.latency_ms,
    }
}

/// The endpoints and identity this desk uses, so the UI carries no host of
/// its own (rules in `app::endpoints`).
#[tauri::command]
pub fn get_endpoints(app: tauri::AppHandle) -> crate::app::endpoints::Endpoints {
    use tauri::Manager;
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    crate::app::endpoints::endpoints(&dir)
}
