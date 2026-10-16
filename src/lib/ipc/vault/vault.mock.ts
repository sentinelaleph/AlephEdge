/**
 * Dev-only in-memory vault that unlocks for any password; reached only
 * through `devMock` in ../bridge, which keeps it out of the release bundle.
 */

import type { AddCredentialInput, CredentialMeta, VaultStatus } from "./vault";

export const mock = (() => {
  let created = false;
  let unlocked = false;
  let idle = 30;
  let pendingPem: string | null = null;
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
      pendingPem = null;
      return status();
    },
    reset() {
      created = false;
      unlocked = false;
      creds.length = 0;
      return status();
    },
    add(input: AddCredentialInput) {
      if (input.keyType === "ed25519") {
        if (!pendingPem) return Promise.reject(new Error("ed25519NoPendingKey"));
        pendingPem = null;
      }
      creds.push({
        exchangeId: input.exchangeId,
        label: input.label,
        permission: input.permission,
        hasPassphrase: Boolean(input.passphrase),
        addedAt: Date.now(),
        keyType: input.keyType ?? "hmac",
        // The preview cannot ask an exchange: IP and creation date unknown.
        ipRestricted: null,
        exchangeCreatedAt: null,
        safety: { noIpRestriction: false, ageDays: 0, ageFrom: "vault", rotate: false },
      });
      return Promise.resolve([...creds]);
    },
    recheck(exchangeId: string, label: string) {
      if (!creds.some((x) => x.exchangeId === exchangeId && x.label === label)) return Promise.reject(new Error("credentialNotFound"));
      return Promise.reject(new Error("keyCheckUnavailable"));
    },
    /** A real keypair from the browser's WebCrypto; only the public half is kept. */
    async ed25519Generate() {
      const pair = (await crypto.subtle.generateKey({ name: "Ed25519" }, false, ["sign", "verify"])) as CryptoKeyPair;
      const spki = new Uint8Array(await crypto.subtle.exportKey("spki", pair.publicKey));
      pendingPem = `-----BEGIN PUBLIC KEY-----\n${btoa(String.fromCharCode(...spki))}\n-----END PUBLIC KEY-----`;
      return pendingPem;
    },
    ed25519Pending: () => Promise.resolve(pendingPem),
    ed25519Discard() {
      pendingPem = null;
      return Promise.resolve();
    },
    list: () => Promise.resolve([...creds]),
    replace(input: AddCredentialInput) {
      const c = creds.find((x) => x.exchangeId === input.exchangeId && x.label === input.label);
      if (!c) return Promise.reject(new Error("credentialNotFound"));
      if (input.keyType === "ed25519") {
        if (!pendingPem) return Promise.reject(new Error("ed25519NoPendingKey"));
        pendingPem = null;
      }
      c.keyType = input.keyType ?? "hmac";
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
