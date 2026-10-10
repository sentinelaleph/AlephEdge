/**
 * DEV-ONLY browser preview of the ghost track. Outside the screenshot harness
 * it answers that the track is unavailable; inside it (?shot=, "Sample data")
 * it pictures an early track (3 of 30 days).
 */

import type { PresetsLive } from "./presetsLive";

const IDS = ["dca_long_classic", "dca_long_safe", "dca_long_quick", "dca_long_btc", "dca_long_alts", "dca_long_stop", "dca_short_classic", "dca_short_safe", "grid_neutral", "grid_neutral_tight", "grid_long_trail", "grid_short"];

export function presetsLive(): PresetsLive {
  if (!new URLSearchParams(window.location.search).has("shot")) throw "liveUnavailable";
  return {
    t0: "2026-10-08T00:00:00Z",
    updatedAt: new Date().toISOString(),
    dataEnd: new Date().toISOString(),
    days: 3,
    minDaysForRanking: 30,
    templates: IDS.map((id, i) => ({
      id,
      returnPct: ((i % 5) - 2) * 0.31,
      bots: 5,
      cycles: 4 + i,
      openCycles: 5,
      worstDdPct: -1 - i * 0.4,
      medianDdPct: -0.5,
      winRateCycles: 1,
      history: [
        { d: "2026-10-08", r: 0, dd: 0 },
        { d: "2026-10-09", r: ((i % 5) - 2) * 0.1, dd: -0.5 },
        { d: "2026-10-10", r: ((i % 5) - 2) * 0.31, dd: -1 },
      ],
    })),
  };
}
