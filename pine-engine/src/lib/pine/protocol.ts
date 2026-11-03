/**
 * Web Worker message protocol. Main thread -> worker: RunRequest; worker ->
 * main thread: RunResponse with the same id. Both are plain, structured-
 * cloneable data (no functions, no AST).
 */
import type { Bar } from "@/lib/ta/core";

import { runSync, type RunOptions, type RunResult } from "./engine";

export interface RunRequest {
  id: number;
  source: string;
  bars: Bar[];
  options: RunOptions;
}

export interface RunResponse {
  id: number;
  result: RunResult;
}

/** The worker's whole job, as a pure function (unit-tested without a Worker). */
export function handleRunRequest(req: RunRequest): RunResponse {
  return { id: req.id, result: runSync(req.source, req.bars, req.options) };
}
