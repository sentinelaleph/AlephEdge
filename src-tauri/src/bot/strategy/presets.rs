//! Templates with their historical-simulation verdict embedded as data.
//! Nothing here is a track record: every figure is a botsim replay on 1h bars
//! with the cost model and fill rule stated alongside it.
//!
//! `dca_long_classic` is the original research preset. The other eleven are
//! beginner templates (owner request 2026-10-03), fixed before any run and
//! judged once by the same four checks (a pre-registration kept with the
//! internal research harness). Every template
//! ships with its verdict: "presetReady", or "failed" plus the checks it
//! failed; the app asks before a failed one is used.

use serde::Serialize;

use super::model::{
    DcaParams, GridParams, GridRange, MarketKind, RestartPolicy, Side, Spacing, StartCondition,
    StrategyConfig, StrategyParams, CONFIG_SCHEMA_VERSION,
};

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SplitResult {
    /// "train" | "valid" | "test".
    pub split: &'static str,
    /// Mean return per bot-month, % of budget.
    pub mean_per_bot_month_pct: f64,
    pub ci95_low_pct: f64,
    pub ci95_high_pct: f64,
    pub months_positive: u32,
    pub months: u32,
    /// One persistent bot per symbol with a fixed budget: return per day of
    /// committed capital, %. Measured for dca_long_classic only.
    pub one_bot_return_per_day_pct: Option<f64>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalSimulation {
    /// Always "historicalSimulation": never shown as a track record.
    pub label: &'static str,
    pub simulator: &'static str,
    pub bar_interval: &'static str,
    pub maker_fee_pct: f64,
    pub taker_fee_pct: f64,
    pub slippage_pct: f64,
    pub fill_rule: &'static str,
    pub splits: Vec<SplitResult>,
    pub test_bots: u32,
    pub test_share_bots_positive: f64,
    pub test_worst_bot_drawdown_pct: f64,
    /// TEST mean at a 0.10% taker fee (dca_long_classic only).
    pub test_mean_at_taker_010_pct: Option<f64>,
    pub test_sign_test_p: f64,
    pub bonferroni_bar: f64,
    /// One-bot-per-symbol worst and median bot drawdown (VALID), %.
    pub one_bot_worst_drawdown_pct: Option<f64>,
    pub one_bot_median_drawdown_pct: Option<f64>,
    /// Longest deal held, days.
    pub max_deal_days: u32,
    /// Deals still open at data end (marked at the last close).
    pub open_deals_at_data_end: u32,
    pub liquidations: u32,
    pub stopped_deals: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Universe {
    /// i18n key of the selection rule.
    pub rule: &'static str,
    /// First rank taken (1 = the most traded pair).
    pub rank_from: u32,
    pub top_n: u32,
    pub volume_window_days: u32,
    pub reselect: &'static str,
    pub exclude: Vec<&'static str>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    /// "presetReady" or "failed".
    pub verdict: &'static str,
    /// Checks a failed template did not pass, in pre-registration order:
    /// negativeSplit, ciIncludesZero, monthsNotAllPositive, liquidations.
    pub fail_reasons: Vec<&'static str>,
    /// i18n key under strategy.preset.situation: the market it is meant for.
    pub situation: &'static str,
    /// Config template; `symbol` is empty: the user picks one from the
    /// universe rule.
    pub config: StrategyConfig,
    /// "onePersistentBotPerSymbol": never a fresh budget per month.
    pub capital_model: &'static str,
    pub universe: Universe,
    pub history: HistoricalSimulation,
}

/// dca_long_classic (= v08): long 1x DCA, 8 safety orders, TP 2%, no stop.
pub fn dca_long_classic() -> Preset {
    Preset {
        id: "dca_long_classic",
        verdict: "presetReady",
        fail_reasons: vec![],
        situation: "bullMajors",
        config: StrategyConfig {
            schema_version: CONFIG_SCHEMA_VERSION,
            name: "DCA Long Classic".into(),
            exchange_id: "binance".into(),
            market: MarketKind::Futures,
            symbol: String::new(),
            side: Side::Long,
            budget: 1000.0,
            leverage: 1,
            start: StartCondition::Immediately,
            restart: RestartPolicy {
                cooldown_min: 0,
                max_cycles: None,
                after_stop: true,
                after_liquidation: false,
                price_band: None,
                end_at_ms: None,
            },
            max_drawdown_pct: None,
            // The simulation had no BTC gate; parity keeps it off.
            pause_on_btc_break: false,
            // Off by default (owner decision 2026-10-01): an engine run of
            // this preset over 2024-10..2026-09 (5 bots, DataHub 1h) gave
            // +41.5% without the breaker and +29.8% with it; one trip closed
            // no-stop DCA cycles at the bottom. The user can switch it on.
            portfolio_breaker: false,
            params: StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 8,
                so_step_pct: 2.5,
                step_scale: 1.3,
                volume_scale: 1.4,
                tp_pct: 2.0,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
            preset_id: Some("dca_long_classic".into()),
        },
        capital_model: "onePersistentBotPerSymbol",
        universe: Universe {
            rule: "topUsdtPerpsByQuoteVolume",
            rank_from: 1,
            top_n: 5,
            volume_window_days: 90,
            reselect: "monthly",
            exclude: vec![
                "USDCUSDT", "BTCDOMUSDT", "FDUSDUSDT", "TUSDUSDT", "BUSDUSDT", "USDPUSDT", "DEFIUSDT",
            ],
        },
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: 2.93,
                    ci95_low_pct: 2.34,
                    ci95_high_pct: 3.44,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: Some(0.054),
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: 1.98,
                    ci95_low_pct: 1.19,
                    ci95_high_pct: 2.99,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: Some(0.022),
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: 1.98,
                    ci95_low_pct: 1.52,
                    ci95_high_pct: 2.46,
                    months_positive: 8,
                    months: 8,
                    one_bot_return_per_day_pct: Some(0.038),
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 1.0,
            test_worst_bot_drawdown_pct: -20.7,
            test_mean_at_taker_010_pct: Some(1.95),
            test_sign_test_p: 0.0039,
            bonferroni_bar: 0.00625,
            one_bot_worst_drawdown_pct: Some(-46.8),
            one_bot_median_drawdown_pct: Some(-16.6),
            max_deal_days: 329,
            open_deals_at_data_end: 5,
            liquidations: 0,
            stopped_deals: 0,
        },
    }
}


// ---- beginner templates (generated from the lab verdicts, 2026-10-03) ----

fn template_config(name: &str, id: &str, side: Side, params: StrategyParams) -> StrategyConfig {
    StrategyConfig {
        schema_version: CONFIG_SCHEMA_VERSION,
        name: name.into(),
        exchange_id: "binance".into(),
        market: MarketKind::Futures,
        symbol: String::new(),
        side,
        budget: 1000.0,
        leverage: 1,
        start: StartCondition::Immediately,
        restart: RestartPolicy {
            cooldown_min: 0,
            max_cycles: None,
            after_stop: true,
            after_liquidation: false,
            price_band: None,
            end_at_ms: None,
        },
        // The simulation had no drawdown stop, BTC gate or breaker.
        max_drawdown_pct: None,
        pause_on_btc_break: false,
        portfolio_breaker: false,
        params,
        preset_id: Some(id.into()),
    }
}

fn template_universe(rule: &'static str, rank_from: u32, top_n: u32) -> Universe {
    Universe {
        rule,
        rank_from,
        top_n,
        volume_window_days: 90,
        reselect: "monthly",
        exclude: vec![
            "USDCUSDT", "BTCDOMUSDT", "FDUSDUSDT", "TUSDUSDT", "BUSDUSDT", "USDPUSDT", "DEFIUSDT",
        ],
    }
}

/// T02 of the template pre-registration (uncertainMajors).
pub fn dca_long_safe() -> Preset {
    Preset {
        id: "dca_long_safe",
        verdict: "presetReady",
        fail_reasons: vec![],
        situation: "uncertainMajors",
        config: template_config(
            "DCA Long Safe",
            "dca_long_safe",
            Side::Long,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 10,
                so_step_pct: 2.5,
                step_scale: 1.2,
                volume_scale: 1.3,
                tp_pct: 1.5,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: 2.19,
                    ci95_low_pct: 1.74,
                    ci95_high_pct: 2.57,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: 1.69,
                    ci95_low_pct: 1.24,
                    ci95_high_pct: 2.17,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: 1.51,
                    ci95_low_pct: 1.16,
                    ci95_high_pct: 1.86,
                    months_positive: 8,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 1.0,
            test_worst_bot_drawdown_pct: -15.8,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.0039,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 330,
            open_deals_at_data_end: 3,
            liquidations: 0,
            stopped_deals: 0,
        },
    }
}

/// T03 of the template pre-registration (quickMajors).
pub fn dca_long_quick() -> Preset {
    Preset {
        id: "dca_long_quick",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit"],
        situation: "quickMajors",
        config: template_config(
            "DCA Long Quick",
            "dca_long_quick",
            Side::Long,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 5,
                so_step_pct: 1.5,
                step_scale: 1.2,
                volume_scale: 1.5,
                tp_pct: 1.0,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: -0.24,
                    ci95_low_pct: -5.93,
                    ci95_high_pct: 5.32,
                    months_positive: 4,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: -6.84,
                    ci95_low_pct: -21.01,
                    ci95_high_pct: 3.16,
                    months_positive: 3,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: 5.63,
                    ci95_low_pct: 4.28,
                    ci95_high_pct: 7.01,
                    months_positive: 8,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.975,
            test_worst_bot_drawdown_pct: -38.2,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.0039,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 654,
            open_deals_at_data_end: 17,
            liquidations: 0,
            stopped_deals: 0,
        },
    }
}

/// T04 of the template pre-registration (btcOnly).
pub fn dca_long_btc() -> Preset {
    Preset {
        id: "dca_long_btc",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit"],
        situation: "btcOnly",
        config: template_config(
            "DCA Long BTC",
            "dca_long_btc",
            Side::Long,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 6,
                so_step_pct: 2.0,
                step_scale: 1.3,
                volume_scale: 1.4,
                tp_pct: 1.5,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("btcOnly", 1, 1),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: 3.04,
                    ci95_low_pct: 2.26,
                    ci95_high_pct: 3.86,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: -0.58,
                    ci95_low_pct: -4.99,
                    ci95_high_pct: 1.89,
                    months_positive: 6,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: 1.94,
                    ci95_low_pct: 1.33,
                    ci95_high_pct: 2.55,
                    months_positive: 8,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 8,
            test_share_bots_positive: 1.0,
            test_worst_bot_drawdown_pct: -16.0,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.0039,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 339,
            open_deals_at_data_end: 2,
            liquidations: 0,
            stopped_deals: 0,
        },
    }
}

/// T05 of the template pre-registration (volatileAlts).
pub fn dca_long_alts() -> Preset {
    Preset {
        id: "dca_long_alts",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "ciIncludesZero", "monthsNotAllPositive"],
        situation: "volatileAlts",
        config: template_config(
            "DCA Long Alts",
            "dca_long_alts",
            Side::Long,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 8,
                so_step_pct: 3.0,
                step_scale: 1.3,
                volume_scale: 1.4,
                tp_pct: 3.0,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 6, 15),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: 1.28,
                    ci95_low_pct: -5.61,
                    ci95_high_pct: 5.18,
                    months_positive: 6,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: 0.16,
                    ci95_low_pct: -3.87,
                    ci95_high_pct: 3.4,
                    months_positive: 5,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: -2.89,
                    ci95_low_pct: -6.38,
                    ci95_high_pct: 0.47,
                    months_positive: 2,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 80,
            test_share_bots_positive: 0.887,
            test_worst_bot_drawdown_pct: -97.2,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.9648,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 663,
            open_deals_at_data_end: 26,
            liquidations: 0,
            stopped_deals: 0,
        },
    }
}

/// T06 of the template pre-registration (limitedLoss).
pub fn dca_long_stop() -> Preset {
    Preset {
        id: "dca_long_stop",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "monthsNotAllPositive"],
        situation: "limitedLoss",
        config: template_config(
            "DCA Long Stop",
            "dca_long_stop",
            Side::Long,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 6,
                so_step_pct: 2.5,
                step_scale: 1.3,
                volume_scale: 1.4,
                tp_pct: 2.0,
                trailing_pct: None,
                sl_pct: Some(25.0),
                max_duration_min: None,
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: 2.82,
                    ci95_low_pct: 0.27,
                    ci95_high_pct: 5.27,
                    months_positive: 6,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: -2.87,
                    ci95_low_pct: -10.18,
                    ci95_high_pct: 3.31,
                    months_positive: 4,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: 2.06,
                    ci95_low_pct: 0.12,
                    ci95_high_pct: 3.7,
                    months_positive: 6,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.925,
            test_worst_bot_drawdown_pct: -26.9,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.1445,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 106,
            open_deals_at_data_end: 4,
            liquidations: 0,
            stopped_deals: 16,
        },
    }
}

/// T07 of the template pre-registration (bearMajors).
pub fn dca_short_classic() -> Preset {
    Preset {
        id: "dca_short_classic",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "ciIncludesZero", "monthsNotAllPositive", "liquidations"],
        situation: "bearMajors",
        config: template_config(
            "DCA Short Classic",
            "dca_short_classic",
            Side::Short,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 6,
                so_step_pct: 2.5,
                step_scale: 1.3,
                volume_scale: 1.4,
                tp_pct: 2.0,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: 1.7,
                    ci95_low_pct: -6.58,
                    ci95_high_pct: 8.8,
                    months_positive: 5,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: 4.75,
                    ci95_low_pct: 4.07,
                    ci95_high_pct: 5.48,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: -8.61,
                    ci95_low_pct: -20.47,
                    ci95_high_pct: 0.87,
                    months_positive: 3,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.675,
            test_worst_bot_drawdown_pct: -100.4,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.8555,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 226,
            open_deals_at_data_end: 11,
            liquidations: 4,
            stopped_deals: 0,
        },
    }
}

/// T08 of the template pre-registration (bearMajorsSafe).
pub fn dca_short_safe() -> Preset {
    Preset {
        id: "dca_short_safe",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "ciIncludesZero", "monthsNotAllPositive", "liquidations"],
        situation: "bearMajorsSafe",
        config: template_config(
            "DCA Short Safe",
            "dca_short_safe",
            Side::Short,
            StrategyParams::Dca(DcaParams {
                base_order: None,
                safety_order: None,
                base_weight: Some(1.0),
                safety_weight: Some(1.0),
                max_so: 8,
                so_step_pct: 3.5,
                step_scale: 1.4,
                volume_scale: 1.3,
                tp_pct: 1.5,
                trailing_pct: None,
                sl_pct: None,
                max_duration_min: None,
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: 3.07,
                    ci95_low_pct: 2.33,
                    ci95_high_pct: 3.69,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: 2.06,
                    ci95_low_pct: 1.73,
                    ci95_high_pct: 2.46,
                    months_positive: 7,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: -6.72,
                    ci95_low_pct: -15.03,
                    ci95_high_pct: -0.14,
                    months_positive: 3,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.625,
            test_worst_bot_drawdown_pct: -100.0,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.8555,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 220,
            open_deals_at_data_end: 13,
            liquidations: 2,
            stopped_deals: 0,
        },
    }
}

/// T09 of the template pre-registration (rangeWide).
pub fn grid_neutral() -> Preset {
    Preset {
        id: "grid_neutral",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "ciIncludesZero", "monthsNotAllPositive"],
        situation: "rangeWide",
        config: template_config(
            "Grid Neutral",
            "grid_neutral",
            Side::Neutral,
            StrategyParams::Grid(GridParams {
                range: GridRange::Relative { lower_pct: 10.0, upper_pct: 10.0 },
                n_grids: 20,
                spacing: Spacing::Geom,
                stop_out_pct: Some(5.0),
                trailing_up: false,
                trail_up_limit: None,
                take_profit_pct: None,
                max_duration_min: Some(20160),
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: -3.0,
                    ci95_low_pct: -5.7,
                    ci95_high_pct: -0.48,
                    months_positive: 2,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: -2.62,
                    ci95_low_pct: -5.56,
                    ci95_high_pct: 0.21,
                    months_positive: 2,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: -2.43,
                    ci95_low_pct: -5.92,
                    ci95_high_pct: 0.85,
                    months_positive: 4,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.475,
            test_worst_bot_drawdown_pct: -34.1,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.6367,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 14,
            open_deals_at_data_end: 3,
            liquidations: 0,
            stopped_deals: 264,
        },
    }
}

/// T10 of the template pre-registration (rangeTight).
pub fn grid_neutral_tight() -> Preset {
    Preset {
        id: "grid_neutral_tight",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "ciIncludesZero", "monthsNotAllPositive"],
        situation: "rangeTight",
        config: template_config(
            "Grid Neutral Tight",
            "grid_neutral_tight",
            Side::Neutral,
            StrategyParams::Grid(GridParams {
                range: GridRange::Relative { lower_pct: 5.0, upper_pct: 5.0 },
                n_grids: 20,
                spacing: Spacing::Geom,
                stop_out_pct: Some(3.0),
                trailing_up: false,
                trail_up_limit: None,
                take_profit_pct: None,
                max_duration_min: Some(10080),
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: -12.13,
                    ci95_low_pct: -16.41,
                    ci95_high_pct: -7.99,
                    months_positive: 0,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: -8.68,
                    ci95_low_pct: -10.65,
                    ci95_high_pct: -6.83,
                    months_positive: 0,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: -6.26,
                    ci95_low_pct: -10.32,
                    ci95_high_pct: -2.72,
                    months_positive: 0,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.325,
            test_worst_bot_drawdown_pct: -50.2,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 1.0,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 7,
            open_deals_at_data_end: 0,
            liquidations: 0,
            stopped_deals: 891,
        },
    }
}

/// T11 of the template pre-registration (slowUptrend).
pub fn grid_long_trail() -> Preset {
    Preset {
        id: "grid_long_trail",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "ciIncludesZero", "monthsNotAllPositive"],
        situation: "slowUptrend",
        config: template_config(
            "Grid Long Trailing",
            "grid_long_trail",
            Side::Long,
            StrategyParams::Grid(GridParams {
                range: GridRange::Relative { lower_pct: 15.0, upper_pct: 10.0 },
                n_grids: 15,
                spacing: Spacing::Geom,
                stop_out_pct: Some(5.0),
                trailing_up: true,
                trail_up_limit: None,
                take_profit_pct: None,
                max_duration_min: Some(43200),
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: -5.38,
                    ci95_low_pct: -13.09,
                    ci95_high_pct: 0.11,
                    months_positive: 3,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: -5.01,
                    ci95_low_pct: -12.32,
                    ci95_high_pct: 2.09,
                    months_positive: 2,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: 1.22,
                    ci95_low_pct: -6.64,
                    ci95_high_pct: 8.17,
                    months_positive: 5,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.6,
            test_worst_bot_drawdown_pct: -40.1,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.3633,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 30,
            open_deals_at_data_end: 4,
            liquidations: 0,
            stopped_deals: 98,
        },
    }
}

/// T12 of the template pre-registration (slowDowntrend).
pub fn grid_short() -> Preset {
    Preset {
        id: "grid_short",
        verdict: "failed",
        fail_reasons: vec!["negativeSplit", "ciIncludesZero", "monthsNotAllPositive"],
        situation: "slowDowntrend",
        config: template_config(
            "Grid Short",
            "grid_short",
            Side::Short,
            StrategyParams::Grid(GridParams {
                range: GridRange::Relative { lower_pct: 10.0, upper_pct: 10.0 },
                n_grids: 20,
                spacing: Spacing::Geom,
                stop_out_pct: Some(5.0),
                trailing_up: false,
                trail_up_limit: None,
                take_profit_pct: None,
                max_duration_min: Some(20160),
            }),
        ),
        capital_model: "onePersistentBotPerSymbol",
        universe: template_universe("topUsdtPerpsByQuoteVolume", 1, 5),
        history: HistoricalSimulation {
            label: "historicalSimulation",
            simulator: "botsim",
            bar_interval: "1h",
            maker_fee_pct: 0.02,
            taker_fee_pct: 0.05,
            slippage_pct: 0.02,
            fill_rule: "conservativeIntrabar",
            splits: vec![
                SplitResult {
                    split: "train",
                    mean_per_bot_month_pct: -1.26,
                    ci95_low_pct: -5.04,
                    ci95_high_pct: 3.3,
                    months_positive: 2,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "valid",
                    mean_per_bot_month_pct: -0.33,
                    ci95_low_pct: -7.25,
                    ci95_high_pct: 6.06,
                    months_positive: 4,
                    months: 7,
                    one_bot_return_per_day_pct: None,
                },
                SplitResult {
                    split: "test",
                    mean_per_bot_month_pct: -5.05,
                    ci95_low_pct: -11.38,
                    ci95_high_pct: 0.09,
                    months_positive: 2,
                    months: 8,
                    one_bot_return_per_day_pct: None,
                },
            ],
            test_bots: 40,
            test_share_bots_positive: 0.375,
            test_worst_bot_drawdown_pct: -48.4,
            test_mean_at_taker_010_pct: None,
            test_sign_test_p: 0.9648,
            bonferroni_bar: 0.00417,
            one_bot_worst_drawdown_pct: None,
            one_bot_median_drawdown_pct: None,
            max_deal_days: 14,
            open_deals_at_data_end: 3,
            liquidations: 0,
            stopped_deals: 264,
        },
    }
}

/// Every shipped template: the ones that passed first, then the rest.
pub fn all() -> Vec<Preset> {
    let mut v = vec![
        dca_long_classic(),
        dca_long_safe(),
        dca_long_quick(),
        dca_long_btc(),
        dca_long_alts(),
        dca_long_stop(),
        dca_short_classic(),
        dca_short_safe(),
        grid_neutral(),
        grid_neutral_tight(),
        grid_long_trail(),
        grid_short(),
    ];
    v.sort_by_key(|p| p.verdict != "presetReady");
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bot::strategy::validate::validate;

    #[test]
    fn preset_is_valid_once_a_symbol_is_picked() {
        for p in all() {
            let mut cfg = p.config.clone();
            assert_eq!(validate(&cfg, 2).err().map(|e| e.code), Some("symbolInvalid"));
            cfg.symbol = "BTCUSDT".into();
            assert_eq!(validate(&cfg, 2), Ok(()));
            assert_eq!(p.history.label, "historicalSimulation");
        }
    }

    /// Pre-registration 2026-10-03: a template ships with its verdict, a
    /// failed one with at least one reason, and the passed ones come first.
    #[test]
    fn every_template_carries_an_honest_verdict() {
        let all = all();
        assert_eq!(all.len(), 12);
        let mut ids: Vec<_> = all.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 12, "ids are unique");
        let first_failed = all.iter().position(|p| p.verdict == "failed").unwrap_or(all.len());
        assert!(all[first_failed..].iter().all(|p| p.verdict == "failed"), "passed ones first");
        for p in &all {
            match p.verdict {
                "presetReady" => {
                    assert!(p.fail_reasons.is_empty(), "{}", p.id);
                    assert!(p.history.splits.iter().all(|s| s.mean_per_bot_month_pct > 0.0), "{}", p.id);
                    assert_eq!(p.history.liquidations, 0, "{}", p.id);
                }
                "failed" => assert!(!p.fail_reasons.is_empty(), "{}", p.id),
                v => panic!("unknown verdict {v}"),
            }
            assert_eq!(p.config.preset_id.as_deref(), Some(p.id));
            assert_eq!(p.config.leverage, 1, "templates are 1x");
        }
        let ready: Vec<_> = all.iter().filter(|p| p.verdict == "presetReady").map(|p| p.id).collect();
        assert_eq!(ready, vec!["dca_long_classic", "dca_long_safe"]);
    }

    #[test]
    fn dca_long_classic_is_outside_the_portfolio_breaker() {
        let p = dca_long_classic();
        assert!(!p.config.portfolio_breaker, "simulated without a breaker (decision 2026-10-01)");
        let mut cfg = p.config.clone();
        cfg.symbol = "ETHUSDT".into();
        assert_eq!(validate(&cfg, 1), Ok(()));
        // The wire carries it, so the form can prefill it off.
        let v = serde_json::to_value(&p.config).unwrap();
        assert_eq!(v["portfolioBreaker"], serde_json::json!(false));
    }

    #[test]
    fn manual_defaults_are_inside_the_portfolio_breaker() {
        use crate::bot::strategy::commands::default_config;
        use crate::bot::strategy::model::StrategyKind;
        for kind in [StrategyKind::Dca, StrategyKind::Grid] {
            assert!(default_config(kind, "binance".into(), "BTCUSDT".into()).portfolio_breaker, "{kind:?}");
        }
    }

    #[test]
    fn a_config_saved_before_the_flag_loads_inside_the_breaker() {
        let mut v = serde_json::to_value(dca_long_classic().config).unwrap();
        v.as_object_mut().unwrap().remove("portfolioBreaker");
        let cfg: StrategyConfig = serde_json::from_value(v).unwrap();
        assert!(cfg.portfolio_breaker, "old rows keep today's behaviour");
    }

    /// Preset parity lives in the form (`matchesPreset`): the flag is part of
    /// it, so switching the breaker on makes the preset's numbers no longer
    /// describe the bot, and leaving it at the preset's value keeps parity.
    #[test]
    fn preset_parity_includes_the_portfolio_breaker() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/pages/Bots/strategy/StrategyForm.tsx");
        let text = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
        let start = text.find("export function matchesPreset").expect("matchesPreset");
        let end = start + text[start..].find("\n}\n").expect("fn end");
        assert!(text[start..end].contains("a.portfolioBreaker === cfg.portfolioBreaker"));
    }
}
