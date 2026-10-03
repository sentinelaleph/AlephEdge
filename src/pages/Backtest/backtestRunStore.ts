/**
 * The backtest run in flight in this window. It lives outside the page so
 * leaving #/backtest and coming back still shows the pending run (Run stays
 * disabled, its progress keeps updating) instead of offering a second run
 * whose progress would be unreadable next to the first. One run at a time:
 * `begin` claims the slot synchronously, so a double click cannot start two.
 */

import type { BacktestProgress } from "@/lib/ipc/strategy/backtest";

export interface RunFlight {
  token: string;
  progress: BacktestProgress | null;
}

export interface RunState {
  flight: RunFlight | null;
  /** Error code of the last finished run ("code|detail"), null on success. */
  error: string | null;
  /** Bumped when a run ends: the stored-runs list reloads. */
  finished: number;
}

let state: RunState = { flight: null, error: null, finished: 0 };
const subs = new Set<() => void>();
let mountedPages = 0;

function set(next: RunState) {
  state = next;
  for (const f of subs) f();
}

export const runStore = {
  get: (): RunState => state,
  subscribe(cb: () => void): () => void {
    subs.add(cb);
    return () => {
      subs.delete(cb);
    };
  },
  /** Claims the run slot for `token`; false while another run is pending. */
  begin(token: string): boolean {
    if (state.flight) return false;
    set({ ...state, flight: { token, progress: null }, error: null });
    return true;
  },
  /** Progress of `token`'s run; ignored for any other (or finished) run. */
  progress(token: string, p: BacktestProgress) {
    if (state.flight?.token !== token || p.runToken !== token) return;
    set({ ...state, flight: { token, progress: p } });
  },
  end(token: string, error: string | null) {
    if (state.flight?.token !== token) return;
    set({ flight: null, error, finished: state.finished + 1 });
  },
  /** The run page is on screen (a finished run may open its report). */
  pageMounted: () => mountedPages > 0,
  mount(): () => void {
    mountedPages += 1;
    return () => {
      mountedPages -= 1;
    };
  },
};

/** Tests only. */
export function resetRunStore() {
  state = { flight: null, error: null, finished: 0 };
  mountedPages = 0;
}
