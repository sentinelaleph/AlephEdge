import type { SkipNote } from "@/lib/ipc/bot/bot";
import i18n from "@/i18n";
import { localizeError } from "@/lib/errorText";

/** A Sentinel veto code ("btc_regime_turn_bearish") in the user's language;
 * anything else (an error code or plain text) goes through localizeError. */
export function localizeDetail(detail: string): string {
  const key = `bots.vetoReason.${detail.trim()}`;
  return i18n.exists(key) ? i18n.t(key) : localizeError(detail);
}

/**
 * `t()` params for one skip note. "vetoClosed" is the only reason whose
 * `detail` packs two values (see `SkipNote.detail` in lib/ipc/bot/bot.ts);
 * every other reason interpolates `detail` verbatim.
 */
export function skipParams(note: SkipNote): Record<string, string> {
  if (note.reason === "vetoClosed") {
    const sep = note.detail?.indexOf("|") ?? -1;
    if (sep >= 0) {
      return { count: note.detail!.slice(0, sep), detail: localizeDetail(note.detail!.slice(sep + 1)) };
    }
  }
  // A detail can itself be an error code ("storeWriteFailed", "fundingFeedError|503"):
  // localised like any other error, plain text passes through unchanged.
  return { detail: note.detail ? localizeDetail(note.detail) : "" };
}

export interface SkipGroup {
  reason: string;
  count: number;
  /** The newest note of the group (its detail is the one shown). */
  latest: SkipNote;
}

/** Groups skip notes by reason, largest group first. */
export function groupSkips(notes: SkipNote[]): SkipGroup[] {
  const map = new Map<string, SkipGroup>();
  for (const n of notes) {
    const g = map.get(n.reason);
    if (!g) map.set(n.reason, { reason: n.reason, count: 1, latest: n });
    else {
      g.count += 1;
      if (n.atMs > g.latest.atMs) g.latest = n;
    }
  }
  return [...map.values()].sort((a, b) => b.count - a.count || b.latest.atMs - a.latest.atMs);
}

/** Skip reasons that mean the risk budget or position slots were full. */
export const BUDGET_REASONS = new Set(["capitalCap", "maxPositions", "botMaxPositions"]);

/** Skip reasons that are a failure of the bot itself, not a filter decision. */
export function isBotFailure(reason: string): boolean {
  return (
    /^live.*(Failed|Unprotected|Unconfirmed|Mismatch|Unavailable)$/.test(reason) ||
    reason === "untrackedExchangePosition" ||
    reason === "storeWriteFailed" ||
    reason === "storeReadFailed" ||
    // Saved bot data restore could not read (it stays on disk, unused).
    reason === "restoreRowUnreadable" ||
    reason === "restoreLiveBlocked" ||
    reason === "restoreConfigInvalid"
  );
}

export interface MinimumOrderNotice {
  /** Notes refused because the sized order was under the exchange minimum. */
  count: number;
  /** The newest such note: its order size (USDT, as Rust formatted it) and pair. */
  orderUsdt: string;
  symbol: string;
}

/**
 * Signals refused with `belowExchangeMinimum` (the order the bot sized was
 * under the 5 USDT exchange minimum), so a bot that only skips says why in
 * one plain line instead of a reason buried in its log. Null when none.
 */
export function minimumOrderNotice(notes: SkipNote[]): MinimumOrderNotice | null {
  let count = 0;
  let latest: SkipNote | null = null;
  for (const n of notes) {
    if (n.reason !== "belowExchangeMinimum") continue;
    count += 1;
    if (!latest || n.atMs > latest.atMs) latest = n;
  }
  if (!latest) return null;
  return { count, orderUsdt: latest.detail ?? "", symbol: latest.symbol };
}
