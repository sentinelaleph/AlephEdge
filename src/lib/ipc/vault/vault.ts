/**
 * Vault IPC bridge (mirrors src-tauri/src/vault). Secrets are write-only from
 * the UI: they go into `vaultAddCredential` and never come back — listings
 * carry metadata only.
 *
 * Browser fallback: an in-memory fake vault so the UI is developable without the
 * Tauri shell. The fake never persists and holds nothing sensitive across a
 * reload — it exists purely so screens render in `npm run dev`.
 */

import { devMock, inTauri, invoke, listenEvent, type Unlisten } from "../bridge";

export type CredentialPermission = "tradeOnly" | "withdrawEnabled" | "unknown";
export type VaultState = "absent" | "locked" | "unlocked";

export interface VaultStatus {
  state: VaultState;
  credentialCount: number;
  /** Minutes of inactivity after which the vault locks itself. Configurable on
   *  the Rust side, so the panel states this rather than a fixed number. */
  idleTimeoutMinutes: number;
}

export interface CredentialMeta {
  exchangeId: string;
  label: string;
  permission: CredentialPermission;
  hasPassphrase: boolean;
  addedAt: number;
  /** Last key renewal (Settings, Renew key); null if never renewed. */
  updatedAt?: number | null;
}

export interface AddCredentialInput {
  exchangeId: string;
  label: string;
  apiKey: string;
  apiSecret: string;
  passphrase?: string;
  permission: CredentialPermission;
}

export function vaultStatus(): Promise<VaultStatus> {
  return inTauri() ? invoke<VaultStatus>("vault_status") : devMock(() => import("./vault.mock"), (m) => m.mock.status());
}

export function vaultCreate(password: string): Promise<VaultStatus> {
  return inTauri() ? invoke<VaultStatus>("vault_create", { password }) : devMock(() => import("./vault.mock"), (m) => m.mock.create());
}

export function vaultUnlock(password: string): Promise<VaultStatus> {
  return inTauri() ? invoke<VaultStatus>("vault_unlock", { password }) : devMock(() => import("./vault.mock"), (m) => m.mock.unlock(password));
}

/**
 * User activity in the window: resets the auto-lock budget of an unlocked
 * vault (Rust applies the expiry first). Callers throttle it.
 */
export function vaultTouch(): Promise<VaultStatus> {
  return inTauri() ? invoke<VaultStatus>("vault_touch") : devMock(() => import("./vault.mock"), (m) => m.mock.status());
}

export function vaultLock(): Promise<VaultStatus> {
  return inTauri() ? invoke<VaultStatus>("vault_lock") : devMock(() => import("./vault.mock"), (m) => m.mock.lock());
}

export function vaultAddCredential(input: AddCredentialInput): Promise<CredentialMeta[]> {
  return inTauri()
    ? invoke<CredentialMeta[]>("vault_add_credential", { input })
    : devMock(() => import("./vault.mock"), (m) => m.mock.add(input));
}

export function vaultListCredentials(): Promise<CredentialMeta[]> {
  return inTauri() ? invoke<CredentialMeta[]>("vault_list_credentials") : devMock(() => import("./vault.mock"), (m) => m.mock.list());
}

export function vaultRemoveCredential(exchangeId: string, label: string): Promise<CredentialMeta[]> {
  return inTauri()
    ? invoke<CredentialMeta[]>("vault_remove_credential", { exchangeId, label })
    : devMock(() => import("./vault.mock"), (m) => m.mock.remove(exchangeId, label));
}

/** Renews the key material of an existing (exchange, label). Same checks as adding. */
export function vaultReplaceCredential(input: AddCredentialInput): Promise<CredentialMeta[]> {
  return inTauri()
    ? invoke<CredentialMeta[]>("vault_replace_credential", { input })
    : devMock(() => import("./vault.mock"), (m) => m.mock.replace(input));
}

export function vaultRenameCredential(exchangeId: string, label: string, newLabel: string): Promise<CredentialMeta[]> {
  return inTauri()
    ? invoke<CredentialMeta[]>("vault_rename_credential", { exchangeId, label, newLabel })
    : devMock(() => import("./vault.mock"), (m) => m.mock.rename(exchangeId, label, newLabel));
}

export function vaultChangePassword(current: string, newPassword: string): Promise<VaultStatus> {
  return inTauri()
    ? invoke<VaultStatus>("vault_change_password", { current, newPassword })
    : devMock(() => import("./vault.mock"), (m) => m.mock.status());
}

/** Auto-lock choices the backend accepts (`IDLE_MINUTE_CHOICES`). */
export const IDLE_MINUTE_CHOICES = [5, 15, 30, 60, 240, 480, 720, 1440, 10080, 43200] as const;

/** An auto-lock budget as words: minutes, hours, a day, a week, a month. */
export function idleDurationKey(minutes: number): { key: string; count: number } {
  if (minutes >= 43200) return { key: "vault.duration.month", count: Math.round(minutes / 43200) };
  if (minutes >= 10080) return { key: "vault.duration.week", count: Math.round(minutes / 10080) };
  if (minutes >= 60) return { key: "vault.duration.hour", count: Math.round(minutes / 60) };
  return { key: "vault.duration.min", count: minutes };
}

export function vaultSetIdleMinutes(minutes: number): Promise<VaultStatus> {
  return inTauri()
    ? invoke<VaultStatus>("vault_set_idle_minutes", { minutes })
    : devMock(() => import("./vault.mock"), (m) => m.mock.setIdle(minutes));
}

/**
 * Fires when the Rust side locks an idle vault on its own.
 *
 * The vault panel reads its status once, on mount — there is no poll to notice a
 * lock that the user did not ask for. Without this the panel would keep showing
 * an unlocked vault and the next action would fail with a bare "vault locked".
 * The payload is the same secret-free status the commands return.
 */
export function onVaultAutoLocked(handler: (status: VaultStatus) => void): Promise<Unlisten> {
  return listenEvent<VaultStatus>("vault:auto-locked", handler);
}

/** The word the reset dialog makes the user type; the backend checks it too. */
export const VAULT_RESET_CONFIRMATION = "RESET";

/** Destroys the vault + keychain salt (no password recovery). Returns Absent. */
export function vaultReset(confirmation: string): Promise<VaultStatus> {
  return inTauri() ? invoke<VaultStatus>("vault_reset", { confirmation }) : devMock(() => import("./vault.mock"), (m) => m.mock.reset());
}

