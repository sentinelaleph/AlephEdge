/**
 * DEV-ONLY in-memory pairing session for the browser preview (no Rust in
 * `npm run dev`). Reached through `devMock`, so the released bundle drops
 * this module and its placeholder QR.
 */

import i18n from "@/i18n";

import type { LinkStateView, PairingView } from "./link";

const MOCK_TTL = 120_000;
let mockStartedAt: number | null = null;
let mockUsed = false;

function mockView(): PairingView | null {
  if (mockStartedAt === null) return null;
  const elapsed = Date.now() - mockStartedAt;
  const remaining = Math.max(0, MOCK_TTL - elapsed);
  if (mockUsed) {
    return {
      qr: null,
      remainingMs: 0,
      ttlMs: MOCK_TTL,
      // The real Rust command returns a raw, un-localized error string here
      // (see src-tauri/src/link/commands.rs) — this dev mock matches that
      // shape but routes through i18next so the dev walkthrough at least
      // respects the language picker instead of leaking one hardcoded locale.
      expiredReason: i18n.t("link.reasonUsed"),
    };
  }
  if (remaining === 0) {
    return {
      qr: null,
      remainingMs: 0,
      ttlMs: MOCK_TTL,
      expiredReason: i18n.t("link.reasonExpired"),
    };
  }
  return {
    qr: "alephedge://pair?relay=ws://localhost:8080/link&desk=desk-dev&k=ZGV2LWFuYWh0YXItb3JuZWstMzJieXRlLXV6dW4",
    remainingMs: remaining,
    ttlMs: MOCK_TTL,
    expiredReason: null,
  };
}


export function begin(): Promise<PairingView> {
  mockStartedAt = Date.now();
  mockUsed = false;
  return Promise.resolve(mockView()!);
}

export function status(): Promise<PairingView | null> {
  return Promise.resolve(mockView());
}

export function cancel(): Promise<void> {
  mockStartedAt = null;
  return Promise.resolve();
}

export function state(): Promise<LinkStateView> {
  return Promise.resolve(mockUsed ? { state: "connected" } : { state: "unpaired" });
}
