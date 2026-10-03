/**
 * Thin bridge to the Rust core. Every IPC module goes through here so the
 * "are we inside Tauri?" check and the dynamic `invoke` import live in one place.
 *
 * In a plain browser (Vite dev without the Tauri shell) `inTauri()` is false and
 * each IPC module supplies its own in-memory mock, so the whole UI is
 * developable and reviewable without building the desktop binary.
 */

import { localizeError } from "@/lib/errorText";

/** True when running inside the Tauri shell (vs. a plain browser). */
export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Runs a browser-preview fallback, in the dev build only.
 *
 * Two of these previews are fail-open by design: the vault opens for any
 * password and membership answers "active, aleph". That is right for
 * `npm run dev` in a plain browser and wrong to ship — `inTauri()` makes them
 * unreachable in the desktop app, but a reader auditing this repository would
 * still find a vault that opens for anyone and a paywall that says yes sitting
 * in the released JavaScript. `import.meta.env.DEV` is substituted at build
 * time, so the released bundle keeps the rejection and drops both the dynamic
 * import and the module behind it: the previews are not merely unreachable
 * there, they are absent (2026-09-20 pre-publication review, S6).
 */
export async function devMock<M, T>(load: () => Promise<M>, run: (mock: M) => Promise<T>): Promise<T> {
  if (!import.meta.env.DEV) {
    throw new Error("No browser fallback in this build. Run the desktop application.");
  }
  return run(await load());
}

/** Invoke a Rust command. Only call when `inTauri()` is true. */
export async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(cmd, args);
}

/** Undoes a subscription made with `listenEvent`. */
export type Unlisten = () => void;

/**
 * Subscribes to an event the Rust side pushes.
 *
 * Every other channel in this app is the UI asking a question. This one exists
 * because the vault can lock itself while nobody is asking (auto-lock), and a
 * panel that keeps showing an unlocked vault until the next action fails is a
 * security control the user cannot see working.
 *
 * Outside the Tauri shell there is nothing to listen to, so this resolves to a
 * no-op rather than throwing — a browser preview has no backend to push.
 */
export async function listenEvent<T>(event: string, handler: (payload: T) => void): Promise<Unlisten> {
  if (!inTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlisten = await listen<T>(event, (e) => handler(e.payload));
  return () => unlisten();
}

/**
 * Extracts a human message from a rejected IPC call. Tauri rejects a
 * `Result<_, String>` command with the raw STRING (not an `Error`), so a plain
 * `e.message` / `instanceof Error` check would hide the real reason behind a
 * generic fallback. Handles string | Error | anything.
 *
 * Rust rejects with stable codes (`vaultWrongPassword`, `code|detail`); a code
 * known under `errors.*` comes back in the user's language (localizeError).
 * Codes owned by other namespaces (`strategy.errors.*`, `backtest.errors.*`)
 * are not under `errors.*`, so they pass through raw for their own renderers.
 */
export function errorMessage(e: unknown, fallback: string): string {
  if (typeof e === "string" && e.trim()) return localizeError(e);
  if (e instanceof Error && e.message) return localizeError(e.message);
  return fallback;
}
