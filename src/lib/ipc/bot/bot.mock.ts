/**
 * DEV-ONLY in-memory bot desk for the browser preview (no Rust in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. It never trades anything and invents no position or skip.
 */

import { DEFAULT_RISK_PCT, MAX_RISK_PCT, MIN_RISK_PCT } from "@/lib/sizing";
import { customPctValid, MAX_CUSTOM_TP_PCT, MIN_CUSTOM_TP_PCT } from "@/lib/takeProfit";
import { LIVE_CONFIRMATION, type BotConfig, type BotDeskStatus, type BotKind } from "./bot";

export const mock = (() => {
  const state: BotDeskStatus = {
    liveTradingEnabled: false,
    binanceIsProduction: true,
    futures: null,
    spot: null,
    pump: null,
    futuresRunning: false,
    spotRunning: false,
    pumpRunning: false,
    openPositions: [],
    recentSkips: [],
    killSwitchTripped: false,
    btcRegime: "unknown",
  };
  const clone = () => Promise.resolve({ ...state, openPositions: [...state.openPositions], recentSkips: [...state.recentSkips] });
  const configKey = { futures: "futures", spot: "spot", pump: "pump" } as const;
  const runKey = { futures: "futuresRunning", spot: "spotRunning", pump: "pumpRunning" } as const;
  return {
    status: clone,
    configure(config: BotConfig) {
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
      // Like bot_configure: `live` is kept from the stored config, never taken
      // from the form.
      state[configKey[config.kind]] = { ...config, live: state[configKey[config.kind]]?.live ?? false };
      return clone();
    },
    setLive(kind: BotKind, enabled: boolean, confirmation: string) {
      const cfg = state[configKey[kind]];
      if (!cfg) return Promise.reject("save the bot's settings first");
      // The browser preview is a simulation-only build, like bot_set_live
      // with LIVE_TRADING_ENABLED off.
      if (enabled) {
        if (confirmation.trim() !== LIVE_CONFIRMATION) {
          return Promise.reject(`type ${LIVE_CONFIRMATION} to confirm`);
        }
        return Promise.reject("this build is simulation-only");
      }
      state[configKey[kind]] = { ...cfg, live: false };
      return clone();
    },
    start(kind: BotKind) {
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
    defaultConfig(kind: BotKind, exchangeId: string): Promise<BotConfig> {
      return Promise.resolve({
        kind,
        exchangeId,
        maxPositions: 3,
        capital: 100,
        leverage: kind === "spot" ? 1 : 3,
        symbols: [],
        combos: [],
        engines: [],
        sizing: "risk",
        riskPerTradePct: DEFAULT_RISK_PCT,
        takeProfit: { kind: "tp1" },
        takeProfitOverrides: {},
        maxSignalAgeMin: 240,
        live: false,
      });
    },
  };
})();
