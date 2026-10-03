/**
 * Phone-link IPC bridge (mirrors src-tauri/src/link).
 *
 * The desk shows a QR; the phone scans it and becomes a remote control. The
 * pairing key travels inside that QR, which is why a QR on screen is a
 * standing credential — the Rust side keeps the session short-lived and
 * single-use, and this module never invents a QR of its own.
 *
 * The desk id and the relay URL are NOT arguments here. Rust resolves both
 * (`src-tauri/src/link/commands.rs`, from `app::endpoints`), because a UI that
 * can pass a host is a UI that can pass the wrong one — and the pairing session
 * it starts is the one thing in this app that hands out a credential.
 *
 * `npm run dev` gets a dev-only in-memory session through `devMock`; the
 * released bundle has none.
 */

import { devMock, inTauri, invoke } from "../bridge";

type Mock = typeof import("./link.mock");
const mock = <T>(run: (m: Mock) => Promise<T>) => devMock(() => import("./link.mock"), run);

export interface PairingView {
  /** Text to encode in the QR. `null` once the session expired or was used. */
  qr: string | null;
  /** Countdown, ms. The user should see when the QR dies. */
  remainingMs: number;
  ttlMs: number;
  /** Why there is no QR, when there isn't one. */
  expiredReason: string | null;
}

export type LinkStateView =
  | { state: "unpaired" }
  | { state: "connecting" }
  | { state: "connected" }
  // `rename_all` on the Rust enum renames variants only: the field stays snake_case.
  | { state: "retrying"; in_secs: number }
  | { state: "stopped"; message: string };

// ---------------------------------------------------------------------------

export async function beginPairing(): Promise<PairingView> {
  if (!inTauri()) return mock((m) => m.begin());
  return invoke<PairingView>("link_begin_pairing");
}

export async function pairingStatus(): Promise<PairingView | null> {
  if (!inTauri()) return mock((m) => m.status());
  return invoke<PairingView | null>("link_pairing_status");
}

export async function cancelPairing(): Promise<void> {
  if (!inTauri()) return mock((m) => m.cancel());
  return invoke<void>("link_cancel_pairing");
}

export async function linkState(): Promise<LinkStateView> {
  if (!inTauri()) return mock((m) => m.state());
  return invoke<LinkStateView>("link_state");
}

/**
 * True only if the relay this feature depends on answers 2xx. The relay is
 * not deployed yet and the phone app has not shipped, so the browser mock
 * (and any environment without the real Rust command) reports `false` rather
 * than pretending the feature works.
 */
export async function relayHealth(): Promise<boolean> {
  if (!inTauri()) return false;
  return invoke<boolean>("link_relay_health");
}
