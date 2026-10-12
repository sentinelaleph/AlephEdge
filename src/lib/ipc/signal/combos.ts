/**
 * Combo catalog IPC (mirrors src-tauri/src/signal/catalog.rs): the published
 * catalog, so the user picks from real setups carrying their real records.
 *
 * TWO records travel with every combo and both are always rendered; never show
 * only the higher one. `live` is what the published book did; `backtest` is
 * what the retrospective sweep claimed. They can disagree widely, and that gap
 * is the most useful thing a user can know before committing capital.
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
