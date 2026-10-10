/**
 * Preset ghost track (mirrors src-tauri/src/app/presets_live.rs): the 12
 * templates run forward on the server from 8 Oct 2026, published every
 * 6 hours. Read-only: shown next to the test verdict, never acted on.
 */

import { devMock, inTauri, invoke } from "../bridge";

export interface LivePoint {
  d: string;
  r: number;
  dd: number | null;
}

export interface LiveTemplate {
  id: string;
  returnPct: number;
  bots: number;
  cycles: number;
  openCycles: number;
  worstDdPct: number | null;
  medianDdPct: number | null;
  winRateCycles: number | null;
  history: LivePoint[];
}

export interface PresetsLive {
  t0: string;
  updatedAt: string;
  dataEnd: string;
  days: number;
  minDaysForRanking: number;
  templates: LiveTemplate[];
}

export function presetsLive(): Promise<PresetsLive> {
  if (inTauri()) return invoke<PresetsLive>("presets_live");
  return devMock(
    () => import("./presetsLive.mock"),
    async (m) => m.presetsLive(),
  );
}

/** True once the track is long enough to compare templates (the 30-day rule). */
export function liveRankable(live: PresetsLive): boolean {
  return live.days >= live.minDaysForRanking;
}
