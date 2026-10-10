/**
 * Membership IPC bridge (mirrors src-tauri/src/membership). `active` is the
 * single gate the trading path consults — false ⇒ the bot cannot trade.
 *
 * Browser fallback: an in-memory fake that mimics a signed-in "aleph" member so
 * gated screens render in `npm run dev`. The real gate always comes from the
 * Rust core talking to ribqa.com.
 */

import { devMock, inTauri, invoke } from "../bridge";

export type MembershipHealth = "active" | "expiring" | "inactive" | "unknown";

export interface MembershipView {
  authenticated: boolean;
  active: boolean;
  state: MembershipHealth;
  email?: string;
  displayName?: string;
  tier?: string;
  currentPeriodEnd?: string;
  cancelAtPeriodEnd: boolean;
  /** Admin role: full access without a subscription. */
  admin?: boolean;
  /** The billing status as the server sent it ("active", "trialing", ...). */
  billingStatus?: string;
  /** When the app last read the membership from the server (UNIX ms). */
  checkedAtMs?: number;
}

export function membershipStatus(): Promise<MembershipView> {
  return inTauri() ? invoke<MembershipView>("membership_status") : devMock(() => import("./membership.mock"), (d) => d.mock.status());
}

export function membershipLogin(email: string, password: string): Promise<MembershipView> {
  return inTauri()
    ? invoke<MembershipView>("membership_login", { email, password })
    : devMock(() => import("./membership.mock"), (d) => d.mock.login(email));
}

export function membershipLogout(): Promise<MembershipView> {
  return inTauri() ? invoke<MembershipView>("membership_logout") : devMock(() => import("./membership.mock"), (d) => d.mock.logout());
}

export function membershipRefresh(): Promise<MembershipView> {
  return inTauri() ? invoke<MembershipView>("membership_refresh") : devMock(() => import("./membership.mock"), (d) => d.mock.status());
}

/**
 * Called once on app launch: silently restores a session from the OS
 * keychain's persisted refresh token, if any, so the user isn't asked to
 * sign in again every time. Falls back to the anonymous view otherwise.
 */
export function membershipRestore(): Promise<MembershipView> {
  return inTauri() ? invoke<MembershipView>("membership_restore") : devMock(() => import("./membership.mock"), (d) => d.mock.status());
}

/** The signed-out view: bot locked. */
export const ANONYMOUS: MembershipView = {
  authenticated: false,
  active: false,
  state: "unknown",
  cancelAtPeriodEnd: false,
};

