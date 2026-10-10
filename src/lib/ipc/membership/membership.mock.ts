/**
 * Dev-only in-memory membership, kept OUT of the production bundle.
 *
 * It answers a login with "active, aleph" whatever it is handed — right for
 * `npm run dev` without the Rust core, and wrong to ship: a paywall that says
 * yes has no business sitting in released JavaScript, even where `inTauri()`
 * makes it unreachable (2026-09-20 pre-publication review, S6).
 */

import { ANONYMOUS, type MembershipView } from "./membership";

export const mock = (() => {
  let view: MembershipView = { ...ANONYMOUS };
  return {
    status: () => Promise.resolve(view),
    login(email: string) {
      view = {
        authenticated: true,
        active: true,
        state: "active",
        email,
        displayName: email.split("@")[0],
        tier: "aleph",
        cancelAtPeriodEnd: false,
        billingStatus: "active",
        checkedAtMs: Date.now(),
      };
      return Promise.resolve(view);
    },
    logout() {
      view = { ...ANONYMOUS };
      return Promise.resolve(view);
    },
  };
})();
