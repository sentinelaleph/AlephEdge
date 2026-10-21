/**
 * DEV-ONLY in-memory bot desk for the browser preview (no Rust in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. It never trades anything and invents no position or skip.
 */

import { capitalAboveCap, defaultCapitalWithin, defaultRiskPctFor } from "@/lib/botLimits";
import { DEFAULT_RISK_PCT, MAX_RISK_PCT, MIN_RISK_PCT } from "@/lib/sizing";
import { mock as riskMock } from "../risk/risk.mock";
import { customPctValid, MAX_CUSTOM_TP_PCT, MIN_CUSTOM_TP_PCT } from "@/lib/takeProfit";
import { LIVE_CONFIRMATION, PILOT_NOTIONAL_USDT, type BotConfig, type BotDeskStatus, type BotKind, type LiveTerms, type TradeRequest } from "./bot";

/** Binance's minimum order (Rust `EXCHANGE_MIN_ORDER_USDT`). */
const MIN_ORDER_USDT = 5;

export const mock = (() => {
  // One build: the order path exists; the preview has no vault key, so no
  // bot can go live here (bot_set_live answers liveNeedsTradeKey).
  const state: BotDeskStatus = {
    liveTradingEnabled: true,
    liveVenues: ["binance"],
    binanceIsProduction: true,
    futures: null,
    spot: null,
    pump: null,
    tradingview: null,
    futuresRunning: false,
    spotRunning: false,
    pumpRunning: false,
    tradingviewRunning: false,
    openPositions: [],
    recentSkips: [],
    killSwitchTripped: false,
    btcRegime: "unknown",
  };
  const clone = () => Promise.resolve({ ...state, openPositions: [...state.openPositions], recentSkips: [...state.recentSkips] });
  const configKey = { futures: "futures", spot: "spot", pump: "pump", tradingview: "tradingview" } as const;
  const runKey = { futures: "futuresRunning", spot: "spotRunning", pump: "pumpRunning", tradingview: "tradingviewRunning" } as const;
  // Like check_capital_cap in bot/commands.rs, against the preview's risk state.
  const capRefusal = async (capital: number) => {
    const cap = (await riskMock.state()).maxCapitalQuote;
    return capitalAboveCap(capital, cap) ? `botCapitalAboveCap|${cap >= 1 ? Math.floor(cap) : Math.floor(cap * 100) / 100}` : null;
  };
  return {
    status: clone,
    async configure(config: BotConfig) {
      const refused = await capRefusal(config.capital);
      if (refused) return Promise.reject(refused);
      // Mirrors BotConfig::validate() in bot/model.rs so the browser mock
      // rejects an out-of-range risk % exactly like the real command does.
      if (
        (config.sizing ?? "fixed") === "risk" &&
        !(
          (config.riskPerTradePct ?? DEFAULT_RISK_PCT) >= MIN_RISK_PCT &&
          (config.riskPerTradePct ?? DEFAULT_RISK_PCT) <= MAX_RISK_PCT
        )
      ) {
        return Promise.reject(
          `risk per trade must be between ${MIN_RISK_PCT}% and ${MAX_RISK_PCT}%`,
        );
      }
      const targets = [config.takeProfit, ...Object.values(config.takeProfitOverrides ?? {})];
      if (targets.some((tp) => tp?.kind === "custom" && !customPctValid(tp.pct))) {
        return Promise.reject(
          `custom take-profit must be between ${MIN_CUSTOM_TP_PCT}% and ${MAX_CUSTOM_TP_PCT}%`,
        );
      }
      // Like bot_configure: `live` and `mode` are kept from the stored config,
      // never taken from the form.
      const prev = state[configKey[config.kind]];
      // Like tv_trade::normalize_config: margin per position from the budget,
      // the passphrase kept as "set" only (the browser preview hashes nothing).
      let saved: BotConfig = { ...config, live: prev?.live ?? false, mode: prev?.mode ?? "paper" };
      if (config.kind === "tradingview" && config.tv) {
        const { passphrase, ...tv } = config.tv;
        const keep = passphrase === undefined ? prev?.tv?.passphraseSha256 : passphrase.trim() ? "set" : undefined;
        saved = {
          ...saved,
          sizing: "fixed",
          capital: Math.floor((tv.budgetUsdt / Math.max(1, config.maxPositions)) * 100) / 100,
          tv: { ...tv, passphraseSha256: keep },
        };
      }
      state[configKey[config.kind]] = saved;
      return clone();
    },
    setLive(kind: BotKind, enabled: boolean, confirmation: string) {
      const cfg = state[configKey[kind]];
      if (!cfg) return Promise.reject("save the bot's settings first");
      // The browser preview holds no vault key: like bot_set_live without a
      // verified trade-only key.
      if (enabled) {
        if (kind !== "futures" && kind !== "tradingview") return Promise.reject("liveFuturesBinanceOnly");
        if (confirmation.trim() !== LIVE_CONFIRMATION) return Promise.reject(`liveConfirmRequired|${LIVE_CONFIRMATION}`);
        return Promise.reject("liveNeedsTradeKey");
      }
      const holdsReal = state.openPositions.some((p) => p.botKind === kind && p.live);
      state[configKey[kind]] = { ...cfg, live: false, mode: holdsReal ? "live" : "paper" };
      state[runKey[kind]] = false;
      return clone();
    },
    liveTerms(kind: BotKind): Promise<LiveTerms> {
      const cfg = state[configKey[kind]];
      if (!cfg) return Promise.reject("botSaveFirst");
      // Like live_terms::signal_terms: the TradingView bot risks its budget,
      // and its largest position is the max size per alert.
      const cents = (v: number) => Math.round(v * 100) / 100;
      const lev = Math.max(1, cfg.leverage);
      const margin = cfg.tv ? cents(cfg.tv.budgetUsdt) : cents(cfg.capital * cfg.maxPositions);
      const notional = cfg.tv ? cents(Math.min(cfg.tv.maxSizeUsdt, cfg.tv.budgetUsdt * lev)) : cents(cfg.capital * lev);
      return Promise.resolve({
        venue: cfg.exchangeId,
        marginAtRiskUsdt: margin,
        maxNotionalUsdt: notional,
        maxPositionMarginUsdt: cents(notional / lev),
        leverage: cfg.leverage,
        positions: cfg.maxPositions,
        pilotCount: 3,
        pilotNotionalUsdt: PILOT_NOTIONAL_USDT,
      });
    },
    closeLiveAndPaper(kind: BotKind) {
      const cfg = state[configKey[kind]];
      if (!cfg) return Promise.reject("botSaveFirst");
      // The preview holds no real position; a real row would need the exchange.
      const left = state.openPositions.filter((p) => p.botKind === kind && p.live).length;
      state[runKey[kind]] = false;
      state[configKey[kind]] = { ...cfg, live: false, mode: left > 0 ? "live" : "paper" };
      return left > 0 ? Promise.reject(`closeNotConfirmed|${left}`) : clone();
    },
    stopAllLive() {
      return Promise.resolve({ signalStopped: 0, strategyStopped: 0 });
    },
    async start(kind: BotKind) {
      const cfg = state[configKey[kind]];
      if (!cfg) return Promise.reject("botNotConfigured");
      if (cfg.mode === "live" && !cfg.live) return Promise.reject("liveArmRequired");
      const refused = await capRefusal(cfg.capital);
      if (refused) return Promise.reject(refused);
      state[runKey[kind]] = true;
      return clone();
    },
    stop(kind: BotKind) {
      state[runKey[kind]] = false;
      return clone();
    },
    closePosition(signalId: string, kind: BotKind) {
      // Like bot_close_position: an unknown position is rejected with the
      // same code. The preview desk opens none, so this is the usual answer.
      const i = state.openPositions.findIndex((p) => p.signalId === signalId && p.botKind === kind);
      if (i < 0) return Promise.reject("positionNotFound");
      state.openPositions.splice(i, 1);
      return clone();
    },
    takeSignal(kind: BotKind): Promise<never> {
      // The preview has no market data: a configured bot cannot price the
      // entry, as bot_take_signal answers without a live quote.
      return Promise.reject(state[configKey[kind]] ? "priceUnavailable" : "botNotConfigured");
    },
    trade(r: TradeRequest): Promise<never> {
      // Like bot_preview_trade / bot_open_trade up to the live price, in the
      // same order: the checks that need no market data. The preview has no
      // quote, so a request that passes them ends where Rust would without
      // one.
      const cfg = state[configKey[r.kind]];
      const symbol = r.symbol.trim().toUpperCase();
      const level = (v: number) => Number.isFinite(v) && v > 0;
      const liveBot = !!cfg && (cfg.live === true || cfg.mode === "live");
      const refusal = !cfg
        ? "botNotConfigured"
        : (r.mode ?? "paper") === "live"
          ? r.kind !== "futures"
            ? "liveFuturesBinanceOnly"
            : !liveBot
              ? "tradeBotIsPaper"
              : "liveArmRequired"
          : liveBot
          ? "manualTakePaperOnly"
          : state.killSwitchTripped
            ? "botDailyLossTripped"
            : r.kind === "pump"
              ? "tradeBotKind"
              : r.kind === "spot" && r.direction === "short"
                ? "spotLongOnly"
                : !/^[A-Z0-9]+USDT$/.test(symbol) || symbol === "USDT"
                  ? "tradeSymbolInvalid"
                  : !level(r.takeProfit) || !level(r.stopLoss)
                    ? "tradeLevelsInvalid"
                    : r.notionalUsdt !== undefined && !level(r.notionalUsdt)
                      ? "tradeSizeInvalid"
                      : r.notionalUsdt !== undefined && r.notionalUsdt < MIN_ORDER_USDT
                        ? `belowExchangeMinimum|${r.notionalUsdt.toFixed(2)}`
                        : state.openPositions.some((p) => p.botKind === r.kind && p.symbol.toUpperCase() === symbol)
                          ? "marketHeld"
                          : "priceUnavailable";
      return Promise.reject(refusal);
    },
    async defaultConfig(kind: BotKind, exchangeId: string): Promise<BotConfig> {
      const cap = (await riskMock.state()).maxCapitalQuote;
      const capital = defaultCapitalWithin(cap);
      return Promise.resolve({
        kind,
        exchangeId,
        maxPositions: 3,
        capital,
        leverage: kind === "spot" ? 1 : 3,
        symbols: [],
        combos: [],
        engines: [],
        sizing: "risk",
        riskPerTradePct: defaultRiskPctFor(capital),
        takeProfit: { kind: "tp1" },
        takeProfitOverrides: {},
        maxSignalAgeMin: 240,
        live: false,
        ...(kind === "tradingview"
          ? {
              sizing: "fixed" as const,
              tv: { name: "TradingView", budgetUsdt: 100, maxSizeUsdt: 50, maxOpensPerDay: 10, staleS: 30, priceGuardPct: 0.5, requirePrice: true },
            }
          : {}),
      });
    },
  };
})();
