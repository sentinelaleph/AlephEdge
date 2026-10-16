import type { LiveTemplate } from "@/lib/ipc/strategy/presetsLive";
import type { Preset } from "@/lib/ipc/strategy/strategy";

/**
 * Whether the chosen pair is on the template's pair list this month. The
 * list comes from the ghost track (the server ranks it exactly as the
 * historical simulation did); without it the app cannot tell.
 */
export type PairCheck =
  | { state: "unknown" }
  | { state: "inList" | "outside"; month: string; list: string[] };

export function presetPairCheck(preset: Preset, symbol: string, live: LiveTemplate | null): PairCheck {
  const sym = symbol.trim().toUpperCase();
  if (preset.universe.rule === "btcOnly" || !sym || !live || live.symbols.length === 0 || !live.universeMonth) {
    return { state: "unknown" };
  }
  return {
    state: live.symbols.includes(sym) ? "inList" : "outside",
    month: live.universeMonth,
    list: live.symbols,
  };
}

/** "BTC, ETH, SOL": the list without the quote currency, as people say it. */
export function pairListText(list: string[]): string {
  return list.map((s) => s.replace(/USDT$/, "")).join(", ");
}
