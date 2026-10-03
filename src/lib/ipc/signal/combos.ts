/**
 * Combo catalog IPC (mirrors src-tauri/src/signal/catalog.rs).
 *
 * The desk's combo filter used to be a free-text box: you could only use it if
 * you already knew both the internal id AND whether the combo was worth
 * trading. This brings the published catalog into the app so the user picks
 * from real setups carrying their real records.
 *
 * TWO records travel with every combo and both are rendered. `live` is what the
 * published book did; `backtest` is what the retrospective sweep claimed. On
 * this project they disagree badly — stophunt_snap read 80.8% live against
 * 58.5% backtest on 2026-08-12 — and that gap is the most useful thing a user
 * can know before committing capital. Showing only the higher one would repeat
 * the failure that killed the mirror-short branch (76.2% retrospective,
 * 41.8% forward).
 */

import { devMock, inTauri, invoke } from "../bridge";

export interface ComboRecord {
  /** Sample size. Never render the rate without this. */
  n: number;
  /** Fraction in [0,1], not a percentage. */
  winRate: number;
  avgPnl: number;
}

export interface ComboEntry {
  id: string;
  name: string;
  description: string;
  /** Human-readable trigger sequence. */
  sequenceHuman: string;
  /** "best" | "candidate" | "disabled", set by the server's own gate. */
  status: string;
  live: ComboRecord;
  backtest: ComboRecord;
}

export function fetchComboCatalog(): Promise<ComboEntry[]> {
  return inTauri() ? invoke<ComboEntry[]>("combo_catalog") : devMock(() => import("./combos.mock"), (m) => Promise.resolve(m.MOCK));
}
