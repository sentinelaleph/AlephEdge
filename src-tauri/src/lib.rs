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
use app::update::{app_check_update, app_install_update};
use bot::strategy::commands::{
    strategy_archive, strategy_close, strategy_close_all, strategy_create, strategy_cycles,
    strategy_default, strategy_detail, strategy_equity, strategy_export_csv, strategy_fills,
    strategy_list, strategy_notes, strategy_orders, strategy_pnl, strategy_presets, strategy_preview,
    strategy_risk_get, strategy_risk_set, strategy_start, strategy_stats, strategy_stop,
    strategy_update, strategy_validate,
};
use bot::strategy::backtest_commands::{
    backtest_delete, backtest_delete_all, backtest_get, backtest_list, backtest_run,
};
use bot::strategy::StrategyManager;
use bot::{
    bot_close_position, bot_configure, bot_default_config, bot_end_pilot, bot_preview_take, bot_set_live, bot_start,
    bot_status, bot_stop, bot_take_signal, BotManager,
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
use store::{skips_export_csv, trades_export_csv, trades_list, trades_stats, StoreManager};
use tauri::Manager;
use vault::{
    spawn_idle_watchdog, vault_add_credential, vault_create, vault_list_credentials, vault_lock,
    vault_remove_credential, vault_set_idle_minutes, vault_change_password, vault_rename_credential, vault_replace_credential, vault_reset, vault_status, vault_touch, vault_unlock, VaultManager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let context = tauri::generate_context!();
    // Keychain entries are per build (release, TESTNET, e2e): set before any
    // command can read the vault salt or the saved session.
    app::keychain::init(&context.config().identifier);
    tauri::Builder::default()
        // FIRST plugin: a second launch hands over to this window and exits
        // before any other plugin or the setup below runs. Keyed on the app
        // identifier, so TESTNET and release still run side by side.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().ok();
            // Second guard, before anything reads or writes edge.db: an
            // exclusive lock on the data dir, held for the life of the process.
            if let Some(dir) = data_dir.as_deref() {
                match app::instance::acquire(dir, app::instance::HANDOFF_WAIT) {
                    Ok(lock) => {
                        app.manage(lock);
                    }
                    Err(e) if e == "alreadyRunning" => {
                        // Before any restore: this process never touched the
                        // database. The other window stays the desk.
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.hide();
                        }
                        eprintln!("Aleph Edge is already running on {}", dir.display());
                        std::process::exit(0);
                    }
                    // The dir cannot hold a lock file: the single-instance
                    // plugin above still guards; carry on without the second
                    // guard rather than refuse to open.
                    Err(e) => eprintln!("instance lock unavailable: {e}"),
                }
            }
            app::window::fit_to_screen(app);
            app.manage(RiskManager::load(data_dir));
            // Reload open paper positions persisted by the previous run.
            app.state::<BotManager>().restore(app.handle().clone());
            // DCA / Grid bots: paper bots resume as they were; a bot on real
            // money comes back Stopped (its open cycle stays managed).
            bot::strategy::engine::restore(app.handle());
            // Real-money state of DCA / Grid bots (bot/strategy_live.rs).
            bot::strategy_live::restore(app.handle());
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
        .manage(bot::strategy_live::StrategyLiveManager::new())
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
            app::about::app_info,
            app::about::open_help_link,
            app::about::open_data_dir,
            app_check_update,
            app_install_update,
            vault_status,
            vault_create,
            vault_unlock,
            vault_lock,
            vault_touch,
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
            skips_export_csv,
            bot_configure,
            bot_set_live,
            bot_end_pilot,
            bot_default_config,
            bot_start,
            bot_stop,
            bot_status,
            bot_close_position,
            bot_take_signal,
            bot_preview_take,
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
            bot::strategy_live::strategy_live_status,
            bot::strategy_live::strategy_set_live,
            bot::strategy_live::strategy_end_pilot,
            bot::strategy_live::strategy_live_report,
            app::preflight::live_preflight,
            app::market_trend::market_trend,
            app::presets_live::presets_live,
            strategy_archive,
            strategy_cycles,
            strategy_orders,
            strategy_fills,
            strategy_equity,
            strategy_stats,
            strategy_pnl,
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
        .run(context)
        .expect("error while running Aleph Edge");
}
