/**
 * Dev-only in-memory vault, kept OUT of the production bundle.
 *
 * It answers `unlock` with success whatever password it is handed, which is
 * exactly right for `npm run dev` in a plain browser and exactly wrong to ship:
 * a reader auditing this repository would find a vault that opens for anyone
 * sitting in the released JavaScript, even though `inTauri()` makes it
 * unreachable there. It now lives behind a dynamic import that only the dev
 * build ever reaches, so the released bundle does not contain it at all
 * (2026-09-20 pre-publication review, S6).
 */

import type { AddCredentialInput, CredentialMeta, VaultStatus } from "./vault";

export const mock = (() => {
  let created = false;
  let unlocked = false;
  let idle = 30;
  const creds: CredentialMeta[] = [];
  const status = (): Promise<VaultStatus> =>
    Promise.resolve({
      state: !created ? "absent" : unlocked ? "unlocked" : "locked",
      credentialCount: unlocked ? creds.length : 0,
      // The dev preview has no watchdog behind it; it reports the shipped
      // default so the panel copy renders at its real length.
      idleTimeoutMinutes: idle,
    });
  return {
    status,
    create() {
      created = true;
      unlocked = true;
      return status();
    },
    unlock(_password: string) {
      if (!created) return Promise.reject(new Error("vault file unreadable"));
      unlocked = true;
      return status();
    },
    lock() {
      unlocked = false;
      return status();
    },
    reset() {
      created = false;
      unlocked = false;
      creds.length = 0;
      return status();
    },
    add(input: AddCredentialInput) {
      creds.push({
        exchangeId: input.exchangeId,
        label: input.label,
        permission: input.permission,
        hasPassphrase: Boolean(input.passphrase),
        addedAt: Date.now(),
      });
      return Promise.resolve([...creds]);
    },
    list: () => Promise.resolve([...creds]),
    replace(input: AddCredentialInput) {
      const c = creds.find((x) => x.exchangeId === input.exchangeId && x.label === input.label);
      if (!c) return Promise.reject(new Error("credentialNotFound"));
      c.updatedAt = Date.now();
      c.hasPassphrase = Boolean(input.passphrase);
      return Promise.resolve([...creds]);
    },
    rename(exchangeId: string, label: string, newLabel: string) {
      const c = creds.find((x) => x.exchangeId === exchangeId && x.label === label);
      if (!c) return Promise.reject(new Error("credentialNotFound"));
      c.label = newLabel.trim();
      return Promise.resolve([...creds]);
    },
    setIdle(minutes: number) {
      idle = minutes;
      return status();
    },
    remove(exchangeId: string, label: string) {
      const i = creds.findIndex((c) => c.exchangeId === exchangeId && c.label === label);
      if (i >= 0) creds.splice(i, 1);
      return Promise.resolve([...creds]);
    },
  };
})();
