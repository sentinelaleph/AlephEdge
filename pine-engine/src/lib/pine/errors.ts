/**
 * Pine-lite errors. Every problem the engine reports, at compile time or at
 * run time, is a PineError: a plain, serialisable object (it crosses the Web
 * Worker boundary) with a kind, a short message and the 1-based line/column
 * of the construct it is about.
 */

export type PineErrorKind =
  /** The source does not lex or parse. */
  | "syntax"
  /** Valid Pine that Pine-lite v1 does not run (named in the message). */
  | "unsupported"
  /** Undeclared or misused names. */
  | "name"
  /** Type / qualifier problems the checker can prove. */
  | "type"
  /** Something failed while bars were executing. */
  | "runtime"
  /** A hard limit (bars, wall time, nodes, loop, plots, source size). */
  | "limit"
  /** An input override that does not fit the declared input. */
  | "input"
  /** Not an error: a compile-time warning (warnings list only). */
  | "warning";

export interface PineError {
  kind: PineErrorKind;
  message: string;
  line: number;
  col: number;
  hint?: string;
}

/** "line 12:5 — ta.mode is not supported in Pine-lite v1" */
export function formatPineError(e: PineError): string {
  return `line ${e.line}:${e.col} — ${e.message}`;
}

/** Thrown inside the engine; always caught at the API boundary and returned as data. */
export class PineFailure extends Error {
  readonly error: PineError;
  constructor(error: PineError) {
    super(formatPineError(error));
    this.error = error;
  }
}

export function pineError(kind: PineErrorKind, message: string, line: number, col: number, hint?: string): PineError {
  return hint === undefined ? { kind, message, line, col } : { kind, message, line, col, hint };
}

export function fail(kind: PineErrorKind, message: string, at: { line: number; col: number }, hint?: string): never {
  throw new PineFailure(pineError(kind, message, at.line, at.col, hint));
}

/**
 * Converts anything thrown inside the engine into a PineError. A JS stack
 * overflow or an unexpected exception is reported as an internal runtime
 * error at 1:1 rather than escaping as a crash.
 */
export function toPineError(e: unknown): PineError {
  if (e instanceof PineFailure) return e.error;
  if (e instanceof RangeError) return pineError("limit", "Script is nested too deeply to evaluate", 1, 1);
  const msg = e instanceof Error ? e.message : String(e);
  return pineError("runtime", `Internal Pine-lite error: ${msg}`, 1, 1);
}

export const NOT_IN_V1 = "is not supported in Pine-lite v1";
