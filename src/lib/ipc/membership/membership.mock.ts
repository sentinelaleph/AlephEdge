/**
 * Dev-only in-memory membership that answers any login with "active, aleph";
 * reached only through `devMock` in ../bridge, which keeps it out of the
 * release bundle.
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
