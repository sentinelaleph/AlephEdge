import { useEffect, useRef, useState } from "react";
import { errorMessage } from "@/lib/ipc/bridge";
import { onPineResult, pineCompile, pineResults, pineStatus, type PineCompileInfo, type PineResult, type PineStatus } from "@/lib/ipc/pine/pine";

/** Wait after the last keystroke before a compile. */
export const COMPILE_DEBOUNCE_MS = 400;
const STATUS_POLL_MS = 5_000;
const RESULTS_POLL_MS = 30_000;

export interface PineCompileState {
  /** The compile of the text on screen, or null while it runs (or the text is empty). */
  info: PineCompileInfo | null;
  /** The last compile that succeeded (its inputs and triggers stay shown while the next one runs). */
  last: PineCompileInfo | null;
  /** A refusal before the engine ran (`pineScriptTooLarge|500`), localized. */
  refused: string | null;
  busy: boolean;
}

/**
 * Compiles the script text (Rust `pine_compile`, nothing saved) once typing
 * pauses. A reply for an older text is dropped, so `info` always belongs to
 * the text on screen.
 */
export function usePineCompile(script: string, fallback: string): PineCompileState {
  const [st, setSt] = useState<PineCompileState>({ info: null, last: null, refused: null, busy: false });
  const seq = useRef(0);
  useEffect(() => {
    const mine = ++seq.current;
    if (!script.trim()) {
      setSt((s) => ({ info: null, last: s.last, refused: null, busy: false }));
      return;
    }
    setSt((s) => ({ info: null, last: s.last, refused: null, busy: true }));
    const timer = window.setTimeout(() => {
      pineCompile(script)
        .then((info) => mine === seq.current && setSt((s) => ({ info, last: info.ok ? info : s.last, refused: null, busy: false })))
        .catch((e) => mine === seq.current && setSt((s) => ({ info: null, last: s.last, refused: errorMessage(e, fallback), busy: false })));
    }, COMPILE_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [script, fallback]);
  return st;
}

/** The bot card (`pine_status`, polled) and the action rows (`pine_results`, on each result event). */
export function usePineDesk(configured: boolean): { status: PineStatus | null; results: PineResult[] | null; error: string | null } {
  const [status, setStatus] = useState<PineStatus | null>(null);
  const [results, setResults] = useState<PineResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    let resultsTimer: number | undefined;
    const tick = () => {
      pineStatus()
        .then((s) => alive && setStatus(s))
        .catch(() => undefined)
        .finally(() => {
          if (alive) timer = window.setTimeout(tick, STATUS_POLL_MS);
        });
    };
    const read = () => {
      window.clearTimeout(resultsTimer);
      pineResults(100)
        .then((r) => {
          if (!alive) return;
          setResults(r);
          setError(null);
        })
        .catch((e) => alive && setError(errorMessage(e, "")))
        .finally(() => {
          if (alive) resultsTimer = window.setTimeout(read, RESULTS_POLL_MS);
        });
    };
    tick();
    read();
    let unlisten: (() => void) | null = null;
    onPineResult(() => read())
      .then((u) => (alive ? (unlisten = u) : u()))
      .catch(() => undefined);
    return () => {
      alive = false;
      window.clearTimeout(timer);
      window.clearTimeout(resultsTimer);
      unlisten?.();
    };
  }, [configured]);
  return { status, results, error };
}
