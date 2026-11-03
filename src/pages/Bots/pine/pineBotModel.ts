import type { BotConfig } from "@/lib/ipc/bot/bot";
import { PINE_LIMITS, type PineAction, type PineCompileInfo, type PineSettings, type PineTimeframe, type PineTriggerInfo } from "@/lib/ipc/pine/pine";
import { parseAllowList } from "@/pages/Bots/tradingview/tvBotModel";
import { inputInfo, type PineInputInfo } from "./pineInputs";

/**
 * The Pine bot's form and the BotConfig it saves. Rust re-checks every value
 * (`PineSettings::validate`, `pinebot::normalize_config`): these checks only
 * say why Save is off. Stop and take profit: 0 in the form = not set (null).
 */
export interface PineBotForm {
  name: string;
  script: string;
  timeframe: PineTimeframe;
  /** Comma or space separated USDT-M pairs. */
  symbols: string;
  triggers: Record<string, PineAction>;
  inputs: Record<string, unknown>;
  warmupBars: number;
  budgetUsdt: number;
  maxSizeUsdt: number;
  maxPositions: number;
  leverage: number;
  maxOpensPerDay: number;
  stopPct: number;
  takeProfitPct: number;
  stopInput: string;
  targetInput: string;
  staleS: number;
  priceGuardPct: number;
}

/** Why Save is off: an i18n key under botsPine.error and its params. */
export interface PineFormError {
  key: string;
  params?: Record<string, string | number>;
}

export function formFromConfig(cfg: BotConfig | null): PineBotForm {
  const p: Partial<PineSettings> = cfg?.pine ?? {};
  return {
    name: p.name ?? "Pine",
    script: p.script ?? "",
    timeframe: p.timeframe ?? "1h",
    symbols: (p.symbols ?? []).join(", "),
    triggers: { ...(p.triggers ?? {}) },
    inputs: { ...(p.inputs ?? {}) },
    warmupBars: p.warmupBars ?? PINE_LIMITS.warmupDefault,
    budgetUsdt: p.budgetUsdt ?? 100,
    maxSizeUsdt: p.maxSizeUsdt ?? 50,
    maxPositions: cfg?.maxPositions ?? 3,
    leverage: cfg?.leverage ?? 3,
    maxOpensPerDay: p.maxOpensPerDay ?? PINE_LIMITS.opensDefault,
    stopPct: p.stopPct === undefined ? 2 : (p.stopPct ?? 0),
    takeProfitPct: p.takeProfitPct === undefined ? 4 : (p.takeProfitPct ?? 0),
    stopInput: p.stopInput ?? "",
    targetInput: p.targetInput ?? "",
    staleS: p.staleS ?? PINE_LIMITS.staleDefault,
    priceGuardPct: p.priceGuardPct ?? PINE_LIMITS.guardDefault,
  };
}

/** The trigger map Save sends: the compiled script's triggers only, Ignore left out (Rust drops it too). */
export function savedTriggers(map: Record<string, PineAction>, compiled: PineTriggerInfo[] | null): Record<string, PineAction> {
  const ids = compiled ? new Set(compiled.map((t) => t.id)) : null;
  const out: Record<string, PineAction> = {};
  for (const [id, a] of Object.entries(map)) if (a !== "ignore" && (!ids || ids.has(id))) out[id] = a;
  return out;
}

/** The input overrides Save sends: inputs of the compiled script whose value differs from its default. */
export function savedInputs(inputs: Record<string, unknown>, compiled: PineInputInfo[] | null): Record<string, unknown> {
  const byId = compiled ? new Map(compiled.map((i) => [String(i.id), i])) : null;
  const out: Record<string, unknown> = {};
  for (const [id, v] of Object.entries(inputs)) {
    const info = byId?.get(id);
    if (byId && (!info || info.defval === v)) continue;
    out[id] = v;
  }
  return out;
}

const pct = (v: number) => (v > 0 ? v : null);
const title = (v: string) => (v.trim() ? v.trim() : null);

/** The config Save sends. `base`: the stored config or the server's defaults; `compile`: the current script's compile. */
export function configFromForm(f: PineBotForm, base: BotConfig, compile: PineCompileInfo | null): BotConfig {
  const inputs = compile?.ok ? compile.inputs.map(inputInfo).filter((i): i is PineInputInfo => i !== null) : null;
  const pine: PineSettings = {
    name: f.name.trim(),
    script: f.script,
    scriptHash: compile?.hash ?? base.pine?.scriptHash ?? "",
    timeframe: f.timeframe,
    symbols: parseAllowList(f.symbols),
    triggers: savedTriggers(f.triggers, compile?.ok ? compile.triggers : null),
    inputs: savedInputs(f.inputs, inputs),
    warmupBars: f.warmupBars,
    budgetUsdt: f.budgetUsdt,
    maxSizeUsdt: f.maxSizeUsdt,
    maxOpensPerDay: f.maxOpensPerDay,
    stopPct: pct(f.stopPct),
    takeProfitPct: pct(f.takeProfitPct),
    stopInput: title(f.stopInput),
    targetInput: title(f.targetInput),
    staleS: f.staleS,
    priceGuardPct: f.priceGuardPct,
  };
  return {
    ...base,
    kind: "pine",
    maxPositions: f.maxPositions,
    leverage: f.leverage,
    symbols: pine.symbols,
    sizing: "fixed",
    // Rust sets capital = budget / positions; sent the same so the cap check reads it.
    capital: Math.floor((f.budgetUsdt / Math.max(1, f.maxPositions)) * 100) / 100,
    pine,
  };
}

/**
 * The first reason the form cannot be saved (or, `live`, started on real
 * money), or null. `compile`: null while the first compile runs.
 */
export function formError(f: PineBotForm, compile: PineCompileInfo | null, maxLeverage: number | null, live: boolean): PineFormError | null {
  const L = PINE_LIMITS;
  const pos = (v: number) => Number.isFinite(v) && v > 0;
  const int = (v: number, lo: number, hi: number) => Number.isInteger(v) && v >= lo && v <= hi;
  if (!f.name.trim() || f.name.trim().length > 40) return { key: "name" };
  if (!f.script.trim()) return { key: "script" };
  if (f.script.length > L.maxScriptKb * 1024) return { key: "scriptSize", params: { max: L.maxScriptKb } };
  if (!compile) return { key: "compiling" };
  if (!compile.ok) return { key: "compile" };
  // Every trigger on Ignore: the bot would run and never place an order (owner, 2 Nov 2026).
  if (Object.keys(savedTriggers(f.triggers, compile.triggers)).length === 0) return { key: "triggersNone" };
  const symbols = parseAllowList(f.symbols);
  if (symbols.length === 0) return { key: "symbolsMin" };
  if (symbols.length > L.maxSymbols) return { key: "symbolsMax", params: { max: L.maxSymbols } };
  if (symbols.some((s) => !/^[A-Z0-9]+USDT$/.test(s) || s === "USDT")) return { key: "symbol" };
  if (!int(f.warmupBars, L.warmupMin, L.warmupMax)) return { key: "warmup", params: { min: L.warmupMin, max: L.warmupMax } };
  if (!pos(f.budgetUsdt)) return { key: "budget" };
  if (!(f.maxSizeUsdt >= L.sizeMin)) return { key: "size", params: { min: L.sizeMin } };
  if (!int(f.maxPositions, 1, 20)) return { key: "positions" };
  if (!int(f.leverage, 1, maxLeverage ?? 125)) return { key: "leverage", params: { max: maxLeverage ?? 1 } };
  if (!int(f.maxOpensPerDay, 1, L.opensMax)) return { key: "opens", params: { max: L.opensMax } };
  if (f.stopPct > 0 && (f.stopPct < L.stopMin || f.stopPct > L.stopMax)) return { key: "stop", params: { min: L.stopMin, max: L.stopMax } };
  if (f.takeProfitPct > 0 && (f.takeProfitPct < L.targetMin || f.takeProfitPct > L.targetMax))
    return { key: "target", params: { min: L.targetMin, max: L.targetMax } };
  if (!(f.stopPct > 0) && !f.stopInput.trim()) return { key: "stopRequired" };
  if (!(f.takeProfitPct > 0) && !f.targetInput.trim()) return { key: "targetRequired" };
  if (live && !(f.stopPct > 0)) return { key: "liveStop" };
  if (!int(f.staleS, L.staleMin, L.staleMax)) return { key: "stale", params: { min: L.staleMin, max: L.staleMax } };
  if (!(f.priceGuardPct > 0) || f.priceGuardPct > L.guardMax) return { key: "guard", params: { max: L.guardMax } };
  return null;
}

/** "plotshape #1" / "alert #1" for a trigger without a title (ids count from 0). */
export function triggerNumber(id: string): number {
  const n = Number(id.split("#")[1]);
  return Number.isInteger(n) ? n + 1 : 0;
}

/** The first 12 hex digits of a script hash: what the page and the Live arm show. */
export function shortHash(hash: string | null | undefined): string {
  return hash ? hash.slice(0, 12) : "";
}

/** Mapped triggers (not Ignore) of a saved config, in the compile's order when known. */
export function mappedTriggers(p: PineSettings | undefined, compiled: PineTriggerInfo[] | null): { id: string; title: string; action: PineAction }[] {
  if (!p) return [];
  const known = new Map((compiled ?? []).map((t) => [t.id, t.title]));
  const order = (compiled ?? []).map((t) => t.id);
  const ids = Object.keys(p.triggers).sort((a, b) => (order.indexOf(a) + 1 || 1e9) - (order.indexOf(b) + 1 || 1e9));
  return ids.filter((id) => p.triggers[id] !== "ignore").map((id) => ({ id, title: known.get(id) ?? "", action: p.triggers[id] }));
}
