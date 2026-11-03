/**
 * Web Worker entry. User scripts are interpreted here, never on the main
 * thread. Loaded by index.ts via `new Worker(new URL("./worker.ts",
 * import.meta.url), { type: "module" })`.
 */
import { handleRunRequest, type RunRequest, type RunResponse } from "./protocol";

interface WorkerScope {
  onmessage: ((e: MessageEvent<RunRequest>) => void) | null;
  postMessage(message: RunResponse): void;
}

const scope = self as unknown as WorkerScope;

scope.onmessage = (e) => {
  scope.postMessage(handleRunRequest(e.data));
};
