/**
 * DEV-ONLY in-memory Pine bot for the browser preview (no Rust in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module. There is no script runtime here: a compile answers
 * `pineRuntimeMissing`, nothing ever runs, fires or trades, and the results
 * stay empty.
 */

import type { BotConfig } from "../bot/bot";
import { PINE_LIMITS, type PineCompileInfo, type PineResult, type PineSettings, type PineStatus } from "./pine";

/** A plain preview hash (the real one is the script's sha256, from Rust). */
function previewHash(source: string): string {
  let h = 0;
  for (let i = 0; i < source.length; i++) h = (h * 31 + source.charCodeAt(i)) | 0;
  return `preview-${(h >>> 0).toString(16)}`;
}

export function defaultSettings(): PineSettings {
  return {
    name: "Pine",
    script: "",
    scriptHash: "",
    timeframe: "1h",
    symbols: [],
    triggers: {},
    inputs: {},
    warmupBars: PINE_LIMITS.warmupDefault,
    budgetUsdt: 100,
    maxSizeUsdt: 50,
    maxOpensPerDay: PINE_LIMITS.opensDefault,
    stopPct: 2,
    takeProfitPct: 4,
    stopInput: null,
    targetInput: null,
    staleS: PINE_LIMITS.staleDefault,
    priceGuardPct: PINE_LIMITS.guardDefault,
  };
}

/** The subset of `PineSettings::validate` a form most often trips. */
export function settingsRefusal(p: PineSettings): string | null {
  if (!p.script.trim()) return "pineScriptEmpty";
  if (p.symbols.length === 0) return "pineSymbolsMin|1";
  if (p.symbols.length > PINE_LIMITS.maxSymbols) return `pineSymbolsMax|${PINE_LIMITS.maxSymbols}`;
  if (p.warmupBars < PINE_LIMITS.warmupMin || p.warmupBars > PINE_LIMITS.warmupMax) return `pineWarmupRange|${PINE_LIMITS.warmupMin}–${PINE_LIMITS.warmupMax}`;
  if (p.stopPct === null && !p.stopInput?.trim()) return "pineStopRequired";
  if (p.takeProfitPct === null && !p.targetInput?.trim()) return "pineTargetRequired";
  return null;
}

export const mock = (() => {
  let config: BotConfig | null = null;
  let running = false;
  const status = (): PineStatus => {
    const p = config?.pine;
    return {
      configured: config !== null,
      running,
      mode: config ? (config.mode ?? "paper") : null,
      armed: false,
      scriptHash: p?.scriptHash ?? null,
      timeframe: p?.timeframe ?? null,
      halted: null,
      failedInARow: 0,
      opensToday: 0,
      maxOpensPerDay: p?.maxOpensPerDay ?? 0,
      pending: 0,
      pilotLeft: 0,
      nextBarClose: null,
      symbols: (p?.symbols ?? []).map((symbol) => ({
        symbol,
        lastRunAt: null,
        lastBarTime: null,
        barsUsed: null,
        runMs: null,
        lastFired: [],
        lastError: null,
        lastNote: null,
      })),
    };
  };
  return {
    compile(source: string): Promise<PineCompileInfo> {
      return Promise.resolve({
        ok: false,
        error: { kind: "unsupported", message: "pineRuntimeMissing", line: 1, col: 1 },
        title: "",
        overlay: false,
        inputs: [],
        triggers: [],
        hash: previewHash(source),
      });
    },
    defaultConfig(): Promise<BotConfig> {
      const config: BotConfig = {
        kind: "pine",
        exchangeId: "binance",
        maxPositions: 3,
        capital: 33.33,
        leverage: 3,
        symbols: [],
        sizing: "fixed",
        pine: defaultSettings(),
      };
      return Promise.resolve(config);
    },
    getConfig(): Promise<BotConfig | null> {
      return Promise.resolve(config ? structuredClone(config) : null);
    },
    saveConfig(next: BotConfig): Promise<BotConfig> {
      if (next.kind !== "pine") return Promise.reject("pineBotKind");
      if (!next.pine) return Promise.reject("pineSettingsMissing");
      const refused = settingsRefusal(next.pine);
      if (refused) return Promise.reject(refused);
      const symbols = [...new Set(next.pine.symbols.map((s) => s.trim().toUpperCase()).filter(Boolean))];
      const pine = { ...next.pine, symbols, scriptHash: previewHash(next.pine.script) };
      config = { ...next, symbols, pine, capital: Math.floor((pine.budgetUsdt / Math.max(1, next.maxPositions)) * 100) / 100 };
      return Promise.resolve(structuredClone(config));
    },
    start(): Promise<PineStatus> {
      if (!config) return Promise.reject("botNotConfigured");
      running = true;
      return Promise.resolve(status());
    },
    stop(): Promise<PineStatus> {
      running = false;
      return Promise.resolve(status());
    },
    status(): Promise<PineStatus> {
      return Promise.resolve(status());
    },
    results(): Promise<PineResult[]> {
      return Promise.resolve([]);
    },
  };
})();
