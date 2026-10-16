//! Bot settings export / import: one JSON file the user saves and can load
//! on this or another computer.
//!
//! The file holds the signal bots' settings (`BotConfig`, as saved under the
//! store's `bot_config.<kind>`) and every DCA / Grid bot's `StrategyConfig`,
//! plus the app version and the export time. It never holds an API key, the
//! vault, the Telegram token or any other secret: none of those types is in
//! it, and a test walks the JSON for such field names.
//!
//! An import never switches real money on:
//! - signal bot settings replace the saved ones through `bot_configure`'s
//!   checks, with `live` false (a bot that was live is simulated afterwards);
//! - each DCA / Grid bot is ADDED as a new, stopped, simulated bot through
//!   `strategy_create`'s checks (validation, bot limit, budget cap, symbol).
//!   Each gets its own result: created, or refused with its error code.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::bot::model::{BotConfig, BotKind};
use crate::bot::strategy::commands::{budget_limit, new_stopped_bot, persist_new_bot, symbol_in};
use crate::bot::strategy::model::{MarketKind, StrategyBot, StrategyConfig};
use crate::bot::strategy::StrategyManager;
use crate::bot::{apply_config, BotManager, KeepLive};
use crate::exchange::ExchangeManager;
use crate::store::StoreManager;

/// Marks the file as ours.
pub const FORMAT: &str = "aleph-edge-bot-settings";
/// Bumped when a reader of this version could misread a newer file.
pub const FORMAT_VERSION: u32 = 1;
/// A settings file is a few KB; anything far larger is not one.
pub const MAX_IMPORT_BYTES: usize = 1_000_000;

const SIGNAL_KINDS: [BotKind; 3] = [BotKind::Futures, BotKind::Spot, BotKind::Pump];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsExport {
    pub format: &'static str,
    pub format_version: u32,
    pub app_version: String,
    pub exported_at_ms: u64,
    pub signal_bots: Vec<BotConfig>,
    pub strategy_bots: Vec<StrategyConfig>,
}

/// The export of these configs. `live` is written false: the file carries
/// settings, never a real-money switch.
pub fn build_export(app_version: &str, now_ms: u64, signal: Vec<BotConfig>, strategy: Vec<StrategyConfig>) -> SettingsExport {
    SettingsExport {
        format: FORMAT,
        format_version: FORMAT_VERSION,
        app_version: app_version.to_string(),
        exported_at_ms: now_ms,
        signal_bots: signal
            .into_iter()
            .map(|mut c| {
                c.live = false;
                c
            })
            .collect(),
        strategy_bots: strategy,
    }
}

/// A read settings file: each entry parsed on its own, so one unreadable
/// bot does not refuse the rest.
#[derive(Debug)]
pub struct ImportFile {
    pub signal_bots: Vec<Result<BotConfig, String>>,
    pub strategy_bots: Vec<Result<StrategyConfig, String>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawFile {
    format: String,
    format_version: u32,
    #[serde(default)]
    signal_bots: Vec<serde_json::Value>,
    #[serde(default)]
    strategy_bots: Vec<serde_json::Value>,
}

fn entry<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T, String> {
    serde_json::from_value(v).map_err(|_| "importEntryUnreadable".to_string())
}

pub fn parse_import(json: &str) -> Result<ImportFile, String> {
    if json.len() > MAX_IMPORT_BYTES {
        return Err("importTooLarge".into());
    }
    let raw: RawFile = serde_json::from_str(json).map_err(|_| "importUnreadable".to_string())?;
    if raw.format != FORMAT {
        return Err("importNotSettings".into());
    }
    if raw.format_version > FORMAT_VERSION {
        return Err(format!("importNewerFormat|{}", raw.format_version));
    }
    Ok(ImportFile {
        signal_bots: raw.signal_bots.into_iter().map(entry).collect(),
        strategy_bots: raw.strategy_bots.into_iter().map(entry).collect(),
    })
}

/// One imported bot's outcome.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportRow {
    /// Signal bots: "futures" / "spot" / "pump". DCA / Grid: "dca" / "grid".
    pub kind: Option<String>,
    pub name: Option<String>,
    pub symbol: Option<String>,
    /// Replaced (signal) or created (DCA / Grid).
    pub ok: bool,
    /// The refusal code when not ok.
    pub code: Option<String>,
    /// The new bot's id (DCA / Grid).
    pub id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub signal_bots: Vec<ImportRow>,
    pub strategy_bots: Vec<ImportRow>,
}

fn strategy_row(cfg: Option<&StrategyConfig>, result: Result<String, String>) -> ImportRow {
    let (ok, code, id) = match result {
        Ok(id) => (true, None, Some(id)),
        Err(code) => (false, Some(code), None),
    };
    ImportRow {
        kind: cfg.map(|c| c.kind().as_str().to_string()),
        name: cfg.map(|c| c.name.trim().to_string()),
        symbol: cfg.map(|c| c.symbol.clone()),
        ok,
        code,
        id,
    }
}

/// Adds the file's DCA / Grid bots in file order, each through
/// `new_stopped_bot` (strategy_create's checks) and `symbol_ok`, then
/// `persist`. `existing` = bots the desk holds; each created one counts
/// toward the limit for the next.
pub fn import_strategies(
    entries: Vec<Result<StrategyConfig, String>>,
    max_leverage: u8,
    mut existing: usize,
    budget_limit: f64,
    now: u64,
    symbol_ok: impl Fn(&StrategyConfig) -> Result<(), String>,
    mut persist: impl FnMut(StrategyBot) -> Result<String, String>,
) -> Vec<ImportRow> {
    entries
        .into_iter()
        .map(|entry| match entry {
            Err(code) => strategy_row(None, Err(code)),
            Ok(cfg) => {
                let made = new_stopped_bot(cfg.clone(), max_leverage, existing, budget_limit, now)
                    .and_then(|bot| symbol_ok(&bot.cfg).map(|()| bot))
                    .and_then(&mut persist);
                if made.is_ok() {
                    existing += 1;
                }
                strategy_row(Some(&cfg), made)
            }
        })
        .collect()
}

/// The signal bot configs to apply, one per kind (a kind listed twice: the
/// second is refused), each with `live` false.
pub fn signal_configs_for_import(entries: Vec<Result<BotConfig, String>>) -> Vec<(ImportRow, Option<BotConfig>)> {
    let mut seen = Vec::new();
    entries
        .into_iter()
        .map(|entry| match entry {
            Err(code) => (ImportRow { kind: None, name: None, symbol: None, ok: false, code: Some(code), id: None }, None),
            Ok(mut cfg) => {
                cfg.live = false;
                let row = ImportRow { kind: Some(cfg.kind.as_str().to_string()), name: None, symbol: None, ok: false, code: None, id: None };
                if seen.contains(&cfg.kind) {
                    return (ImportRow { code: Some("importDuplicateKind".into()), ..row }, None);
                }
                seen.push(cfg.kind);
                (row, Some(cfg))
            }
        })
        .collect()
}

fn now_ms() -> u64 {
    crate::bot::strategy::engine::now_ms()
}

/// The export as pretty JSON; the webview saves it as a file.
#[tauri::command]
pub fn settings_export(app: AppHandle, strategy: State<StrategyManager>) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let store = app.state::<StoreManager>();
    let mut signal = Vec::new();
    for kind in SIGNAL_KINDS {
        if let Some(body) = store.get_meta(&dir, &format!("bot_config.{}", kind.as_str()))? {
            match serde_json::from_str::<BotConfig>(&body) {
                Ok(cfg) if cfg.kind == kind => signal.push(cfg),
                _ => return Err(format!("exportConfigUnreadable|{}", kind.as_str())),
            }
        }
    }
    let bots = strategy.bots().into_iter().map(|b| b.cfg).collect();
    let file = build_export(&app.package_info().version.to_string(), now_ms(), signal, bots);
    serde_json::to_string_pretty(&file).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn settings_import(app: AppHandle, json: String) -> Result<ImportReport, String> {
    let file = parse_import(&json)?;

    let bots = app.state::<BotManager>();
    let signal_bots = signal_configs_for_import(file.signal_bots)
        .into_iter()
        .map(|(row, cfg)| match cfg {
            None => row,
            Some(cfg) => match apply_config(&app, &bots, cfg, KeepLive::No) {
                Ok(()) => ImportRow { ok: true, ..row },
                Err(code) => ImportRow { code: Some(code), ..row },
            },
        })
        .collect();

    // One symbol list per (exchange, market), read before the checks.
    let mut lists: HashMap<(String, bool), Vec<String>> = HashMap::new();
    for cfg in file.strategy_bots.iter().flatten() {
        let key = (cfg.exchange_id.clone(), cfg.market == MarketKind::Futures);
        if let std::collections::hash_map::Entry::Vacant(slot) = lists.entry(key) {
            let (exchange, futures) = slot.key().clone();
            slot.insert(app.state::<ExchangeManager>().symbols(&exchange, futures).await);
        }
    }
    let mgr = app.state::<StrategyManager>();
    let max_leverage = app.state::<crate::risk::RiskManager>().limits().max_leverage;
    let now = now_ms();
    let strategy_bots = import_strategies(
        file.strategy_bots,
        max_leverage,
        mgr.bots().len(),
        budget_limit(&app, &mgr),
        now,
        |cfg| {
            let key = (cfg.exchange_id.clone(), cfg.market == MarketKind::Futures);
            symbol_in(lists.get(&key).map(Vec::as_slice).unwrap_or(&[]), cfg)
        },
        |bot| persist_new_bot(&app, &mgr, bot, now),
    );
    Ok(ImportReport { signal_bots, strategy_bots })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::commands::default_config;
    use crate::bot::strategy::limits::MAX_STRATEGY_BOTS;
    use crate::bot::strategy::model::{BotRunState, StrategyKind};

    const NOW: u64 = 1_792_158_307_000;

    fn sample() -> SettingsExport {
        let mut live = BotConfig::new_default(BotKind::Futures, "binance");
        live.live = true;
        let spot = BotConfig::new_default(BotKind::Spot, "binance");
        let mut grid = default_config(StrategyKind::Grid, "binance".into(), "ETHUSDT".into());
        grid.name = "Grid ETH".into();
        let dca = default_config(StrategyKind::Dca, "binance".into(), "BTCUSDT".into());
        build_export("0.3.14", NOW, vec![live, spot], vec![dca, grid])
    }

    fn keys_of(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, v) in m {
                    out.push(k.clone());
                    keys_of(v, out);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(|v| keys_of(v, out)),
            _ => {}
        }
    }

    #[test]
    fn the_export_holds_no_secret_and_no_real_money_switch() {
        let json = serde_json::to_string_pretty(&sample()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let mut keys = Vec::new();
        keys_of(&v, &mut keys);
        assert!(keys.len() > 20, "{keys:?}");
        for k in &keys {
            let k = k.to_ascii_lowercase();
            for bad in ["key", "secret", "token", "password", "vault"] {
                assert!(!k.contains(bad), "export field {k} looks like a secret");
            }
        }
        assert_eq!(v["format"], FORMAT);
        assert_eq!(v["appVersion"], "0.3.14");
        assert_eq!(v["exportedAtMs"], NOW);
        assert!(v["signalBots"].as_array().unwrap().iter().all(|b| b["live"] == false));
        assert_eq!(v["strategyBots"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn import_round_trip_adds_the_same_configs_as_stopped_paper_bots() {
        let src = sample();
        let mut bad = default_config(StrategyKind::Dca, "binance".into(), "SOLUSDT".into());
        bad.name = String::new();
        let mut file = serde_json::to_value(&src).unwrap();
        file["strategyBots"].as_array_mut().unwrap().push(serde_json::to_value(&bad).unwrap());
        file["strategyBots"].as_array_mut().unwrap().push(serde_json::json!({ "name": "broken" }));
        let parsed = parse_import(&file.to_string()).unwrap();

        let mut saved: Vec<StrategyBot> = Vec::new();
        let rows = import_strategies(parsed.strategy_bots, 3, 0, 1e9, NOW, |_| Ok(()), |b| {
            saved.push(b.clone());
            Ok(b.id)
        });
        assert_eq!(rows.len(), 4);
        assert!(rows[0].ok && rows[1].ok, "{rows:?}");
        assert_eq!(rows[1].name.as_deref(), Some("Grid ETH"));
        assert_eq!(rows[1].kind.as_deref(), Some("grid"));
        let want_bad = crate::bot::strategy::validate::validate(&bad, 3).unwrap_err().to_string();
        assert_eq!(rows[2], ImportRow {
            kind: Some("dca".into()),
            name: Some(String::new()),
            symbol: Some("SOLUSDT".into()),
            ok: false,
            code: Some(want_bad),
            id: None,
        });
        assert_eq!(rows[3].code.as_deref(), Some("importEntryUnreadable"));

        assert_eq!(saved.len(), 2);
        for (bot, cfg) in saved.iter().zip(&src.strategy_bots) {
            assert_eq!(&bot.cfg, cfg, "the imported config is the exported one");
            assert_eq!(bot.state, BotRunState::Stopped);
            assert_eq!(bot.cycles_done, 0);
            assert_eq!(bot.realized_quote, 0.0);
            assert_eq!(Some(&bot.id), rows.iter().find(|r| r.id.as_ref() == Some(&bot.id)).and_then(|r| r.id.as_ref()));
        }

        let signal = signal_configs_for_import(parsed.signal_bots);
        assert_eq!(signal.len(), 2);
        assert!(signal.iter().all(|(_, c)| c.as_ref().is_some_and(|c| !c.live)));
    }

    #[test]
    fn import_respects_the_bot_limit_symbol_check_and_budget_cap() {
        let cfgs = sample().strategy_bots;
        let rows = import_strategies(cfgs.iter().cloned().map(Ok).collect(), 3, MAX_STRATEGY_BOTS - 1, 1e9, NOW, |_| Ok(()), |b| Ok(b.id));
        assert!(rows[0].ok);
        assert_eq!(rows[1].code.as_deref(), Some("maxBots"));

        let rows = import_strategies(cfgs.iter().cloned().map(Ok).collect(), 3, 0, 1e9, NOW, |c| symbol_in(&["BTCUSDT".to_string()], c), |b| Ok(b.id));
        assert!(rows[0].ok);
        assert_eq!(rows[1].code.as_deref(), Some("symbolUnknown|symbol"));

        let rows = import_strategies(cfgs.into_iter().map(Ok).collect(), 3, 0, 10.0, NOW, |_| Ok(()), |b| Ok(b.id));
        assert!(rows.iter().all(|r| r.code.as_deref().is_some_and(|c| c.starts_with("budgetCapReached|budget"))), "{rows:?}");

        // A failed save is that bot's refusal and does not count.
        let rows = import_strategies(sample().strategy_bots.into_iter().map(Ok).collect(), 3, MAX_STRATEGY_BOTS - 1, 1e9, NOW, |_| Ok(()), {
            let mut first = true;
            move |b: StrategyBot| {
                if std::mem::take(&mut first) {
                    Err("storeWriteFailed".into())
                } else {
                    Ok(b.id)
                }
            }
        });
        assert_eq!(rows[0].code.as_deref(), Some("storeWriteFailed"));
        assert!(rows[1].ok);
    }

    #[test]
    fn an_import_refuses_other_files_and_duplicate_kinds() {
        assert_eq!(parse_import("not json").unwrap_err(), "importUnreadable");
        assert_eq!(parse_import(r#"{"format":"x","formatVersion":1}"#).unwrap_err(), "importNotSettings");
        assert_eq!(
            parse_import(&format!(r#"{{"format":"{FORMAT}","formatVersion":2}}"#)).unwrap_err(),
            "importNewerFormat|2"
        );
        assert_eq!(parse_import(&" ".repeat(MAX_IMPORT_BYTES + 1)).unwrap_err(), "importTooLarge");
        let one = BotConfig::new_default(BotKind::Spot, "binance");
        let rows = signal_configs_for_import(vec![Ok(one.clone()), Ok(one), Err("importEntryUnreadable".into())]);
        assert!(rows[0].1.is_some());
        assert_eq!(rows[1].0.code.as_deref(), Some("importDuplicateKind"));
        assert_eq!(rows[2].0.code.as_deref(), Some("importEntryUnreadable"));
    }
}
