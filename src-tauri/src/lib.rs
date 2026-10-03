//! Aleph Edge — Rust core: the composition root.
//!
//! F1 wired the local key **vault** + Sentinel **membership** gate. F2 added
//! the **signal** client (real SSE) and the **exchange** adapter (Binance).
//! F3 adds the **risk** manager, Sentinel **market** data (FR/LD/BTC macro),
//! the SQLite **store**, and the **bot** orchestrator (prechecks, simulated
//! fills — live orders stay behind a flag, OFF through F3). Each subsystem
//! owns its own types, logic, and commands; this file only registers managed
//! state and the command surface, then runs the app.

mod app;
mod bot;
mod exchange;
mod link;
mod market;
mod membership;
mod risk;
mod signal;
mod store;
mod vault;

use app::cockpit::{cockpit_exchange, cockpit_sentinel};
use app::commands::{app_version, get_endpoints, get_health_snapshot};
use bot::strategy::commands::{
    strategy_archive, strategy_close, strategy_close_all, strategy_create, strategy_cycles,
    strategy_default, strategy_detail, strategy_equity, strategy_export_csv, strategy_fills,
    strategy_list, strategy_notes, strategy_orders, strategy_presets, strategy_preview,
    strategy_risk_get, strategy_risk_set, strategy_start, strategy_stats, strategy_stop,
    strategy_update, strategy_validate,
};
use bot::strategy::backtest_commands::{
    backtest_delete, backtest_delete_all, backtest_get, backtest_list, backtest_run,
};
use bot::strategy::StrategyManager;
use bot::{
    bot_close_position, bot_configure, bot_default_config, bot_set_live, bot_start, bot_status,
    bot_stop, BotManager,
};
use exchange::{
    exchange_account, exchange_close_all, exchange_close_position, exchange_list, exchange_status,
    exchange_symbols, ExchangeManager,
};
use market::MarketManager;
use membership::{
    membership_login, membership_logout, membership_refresh, membership_restore, membership_status,
    MembershipManager,
};
use risk::{
    risk_get, risk_set_balance, risk_set_close_on_stop, risk_set_daily_loss, risk_set_level,
    RiskManager,
};
use signal::{
    combo_catalog, signal_connect, signal_disconnect, signal_health, signal_recent, SignalManager,
};
use store::{trades_export_csv, trades_list, trades_stats, StoreManager};
use tauri::Manager;
use vault::{
    spawn_idle_watchdog, vault_add_credential, vault_create, vault_list_credentials, vault_lock,
    vault_remove_credential, vault_set_idle_minutes, vault_change_password, vault_rename_credential, vault_replace_credential, vault_reset, vault_status, vault_unlock, VaultManager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().ok();
            app.manage(RiskManager::load(data_dir));
            // Reload open paper positions persisted by the previous run.
            app.state::<BotManager>().restore(app.handle().clone());
            // DCA / Grid paper bots: all come back Stopped; an open cycle is
            // managed (never re-armed) until the user presses Start.
            bot::strategy::engine::restore(app.handle());
            // BTC regime from launch, not only once a bot runs.
            bot::engine::spawn_regime_poller(app.handle().clone());
            // Auto-lock the vault once it has been idle for its configured
            // budget. Started here, unconditionally and for the life of the
            // process: an unattended desk used to hold decrypted exchange keys
            // in memory from the single password entry until it was closed.
            vault::restore_idle_minutes(app.handle());
            spawn_idle_watchdog(app.handle().clone());
            // Signature-verified auto-update (desktop only).
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .manage(VaultManager::new())
        .manage(MembershipManager::new())
        .manage(ExchangeManager::new())
        .manage(SignalManager::new())
        .manage(MarketManager::new())
        .manage(StoreManager::new())
        .manage(BotManager::new())
        .manage(StrategyManager::new())
        .manage(link::commands::PairingState::new())
        .manage(link::commands::LinkRuntime::new())
        .invoke_handler(tauri::generate_handler![
            link::commands::link_begin_pairing,
            link::commands::link_pairing_status,
            link::commands::link_cancel_pairing,
            link::health::link_relay_health,
            link::commands::link_state,
            get_health_snapshot,
            get_endpoints,
            app_version,
            vault_status,
            vault_create,
            vault_unlock,
            vault_lock,
            vault_add_credential,
            vault_list_credentials,
            vault_remove_credential,
            vault_replace_credential,
            vault_rename_credential,
            vault_change_password,
            vault_set_idle_minutes,
            vault_reset,
            membership_login,
            membership_logout,
            membership_status,
            membership_refresh,
            membership_restore,
            exchange_status,
            exchange_list,
            exchange_symbols,
            exchange_account,
            exchange_close_position,
            exchange_close_all,
            cockpit_exchange,
            cockpit_sentinel,
            signal_connect,
            signal_disconnect,
            signal_recent,
            signal_health,
            combo_catalog,
            risk_get,
            risk_set_level,
            risk_set_balance,
            risk_set_close_on_stop,
            risk_set_daily_loss,
            trades_list,
            trades_stats,
            trades_export_csv,
            bot_configure,
            bot_set_live,
            bot_default_config,
            bot_start,
            bot_stop,
            bot_status,
            bot_close_position,
            strategy_list,
            strategy_detail,
            strategy_default,
            strategy_preview,
            strategy_validate,
            strategy_create,
            strategy_update,
            strategy_start,
            strategy_stop,
            strategy_close,
            strategy_close_all,
            strategy_archive,
            strategy_cycles,
            strategy_orders,
            strategy_fills,
            strategy_equity,
            strategy_stats,
            strategy_export_csv,
            strategy_risk_get,
            strategy_risk_set,
            strategy_notes,
            strategy_presets,
            backtest_run,
            backtest_list,
            backtest_get,
            backtest_delete,
            backtest_delete_all,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aleph Edge");
}
