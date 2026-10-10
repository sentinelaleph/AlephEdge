import { useEffect, useState } from "react";
import { errorMessage } from "../bridge";
import { marketTrend, type MarketTrend } from "./trend";

const REFRESH_MS = 15 * 60_000;

let cached: MarketTrend | null = null;
let inflight: Promise<MarketTrend> | null = null;

function load(): Promise<MarketTrend> {
  if (cached && Date.now() - cached.checkedAtMs < REFRESH_MS) return Promise.resolve(cached);
  inflight ??= marketTrend()
    .then((t) => (cached = t))
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

/** The shared BTC trend reading: one request for every view, refreshed every 15 minutes. */
export function useMarketTrend(): { trend: MarketTrend | null; error: string | null } {
  const [trend, setTrend] = useState<MarketTrend | null>(cached);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    const run = () =>
      load().then(
        (t) => {
          if (!alive) return;
          setTrend(t);
          setError(null);
        },
        (e) => alive && setError(errorMessage(e, "trendUnavailable")),
      );
    void run();
    const timer = window.setInterval(() => void run(), REFRESH_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, []);
  return { trend, error };
}
