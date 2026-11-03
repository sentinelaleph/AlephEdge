import { useEffect, useState } from "react";
import { tvSymbolMinimum } from "@/lib/ipc/tvlink/tvlink";

/** Wait after the last edit of the pairs before asking Binance. */
export const PAIR_MINIMUM_DEBOUNCE_MS = 600;

/**
 * Binance's smallest order (USDT at the live price) for each of the bot's
 * pairs, read through Rust's `tv_symbol_minimum` once the pairs stop
 * changing. A pair Binance did not answer for is left out: no number is
 * shown for it, and the bot's own check still refuses at entry time.
 * Owner, 2 Nov 2026: a 50 USDT size on BTCUSDT (minimum 54.41) was only
 * found out when the first Buy was skipped.
 */
export function usePairMinimums(pairs: string[]): Record<string, number> {
  const key = pairs.join(",");
  const [mins, setMins] = useState<Record<string, number>>({});

  useEffect(() => {
    const list = key ? key.split(",") : [];
    if (list.length === 0) {
      setMins({});
      return;
    }
    let gone = false;
    const timer = window.setTimeout(() => {
      void Promise.all(
        list.map((s) =>
          tvSymbolMinimum(s)
            .then((r) => (r.status === "ok" && r.minUsdt !== null ? ([s, r.minUsdt] as const) : null))
            .catch(() => null),
        ),
      ).then((rows) => {
        if (gone) return;
        setMins(Object.fromEntries(rows.filter((r): r is readonly [string, number] => r !== null)));
      });
    }, PAIR_MINIMUM_DEBOUNCE_MS);
    return () => {
      gone = true;
      window.clearTimeout(timer);
    };
  }, [key]);

  return mins;
}

/** The pairs whose minimum is above the size per entry, highest minimum first. */
export function pairsBelowMinimum(mins: Record<string, number>, sizeUsdt: number): { symbol: string; min: number }[] {
  if (!(sizeUsdt > 0)) return [];
  return Object.entries(mins)
    .filter(([, min]) => min > sizeUsdt + 1e-9)
    .map(([symbol, min]) => ({ symbol, min }))
    .sort((a, b) => b.min - a.min);
}
