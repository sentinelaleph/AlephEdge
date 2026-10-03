import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  onVaultAutoLocked,
  vaultAddCredential,
  vaultChangePassword,
  vaultCreate,
  vaultListCredentials,
  vaultLock,
  vaultRemoveCredential,
  vaultRenameCredential,
  vaultReplaceCredential,
  vaultReset,
  VAULT_RESET_CONFIRMATION,
  vaultSetIdleMinutes,
  vaultStatus,
  vaultUnlock,
  type AddCredentialInput,
  type CredentialMeta,
  type VaultStatus,
} from "@/lib/ipc/vault/vault";

/** Pre-flight placeholder, replaced by the first real status. The timeout is 0
 *  rather than a plausible 30: the only screen that prints it is the unlocked
 *  panel, which never renders before Rust has answered, and a guessed number in
 *  a security claim is worse than an obviously empty one. */
const ABSENT: VaultStatus = { state: "absent", credentialCount: 0, idleTimeoutMinutes: 0 };

export interface VaultController {
  status: VaultStatus;
  credentials: CredentialMeta[];
  initializing: boolean;
  busy: boolean;
  error: string | null;
  create: (password: string) => Promise<void>;
  unlock: (password: string) => Promise<void>;
  lock: () => Promise<void>;
  addKey: (input: AddCredentialInput) => Promise<void>;
  removeKey: (exchangeId: string, label: string) => Promise<void>;
  /** Swaps a stored key for a new one under the same label; the old key stays if this fails. */
  renewKey: (input: AddCredentialInput) => Promise<void>;
  renameKey: (exchangeId: string, label: string, newLabel: string) => Promise<void>;
  changePassword: (current: string, next: string) => Promise<void>;
  setIdleMinutes: (minutes: number) => Promise<void>;
  /** Destroys the vault file + keychain salt so a new one can be created
   * (the only recovery path — there is no password recovery). */
  reset: () => Promise<void>;
}

/** Owns vault state: lifecycle (create/unlock/lock) + credential CRUD. */
export function useVault(): VaultController {
  const { t } = useTranslation();
  const [status, setStatus] = useState<VaultStatus>(ABSENT);
  const [credentials, setCredentials] = useState<CredentialMeta[]>([]);
  const [initializing, setInitializing] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    vaultStatus()
      .then((s) => alive && setStatus(s))
      .catch(() => undefined)
      .finally(() => alive && setInitializing(false));
    return () => {
      alive = false;
    };
  }, []);

  // The vault can lock itself after its idle budget runs out, and nothing here
  // polls for that. Without this subscription the panel would go on showing an
  // unlocked vault until the user's next action failed — an auto-lock the user
  // only ever meets as an error message.
  useEffect(() => {
    let alive = true;
    const pending = onVaultAutoLocked((locked) => {
      if (!alive) return;
      setStatus(locked);
      setCredentials([]);
    });
    return () => {
      alive = false;
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  const run = useCallback(async <T,>(action: () => Promise<T>): Promise<T> => {
    setError(null);
    setBusy(true);
    try {
      return await action();
    } catch (e) {
      setError(errorMessage(e, t("vault.error")));
      // A vault action can fail because the vault locked itself out from under
      // it: the Rust side applies the idle rule before serving any credential,
      // which can beat the watchdog tick that would have pushed the event. So
      // re-read the truth on every failure rather than leaving the panel
      // claiming to be unlocked while the backend has already wiped the keys.
      try {
        const fresh = await vaultStatus();
        setStatus(fresh);
        if (fresh.state !== "unlocked") setCredentials([]);
      } catch {
        /* status unavailable too — keep what we had and show the first error */
      }
      throw e;
    } finally {
      setBusy(false);
    }
  }, [t]);

  const openWith = useCallback(
    async (s: VaultStatus) => {
      setStatus(s);
      setCredentials(await vaultListCredentials());
    },
    [],
  );

  const create = useCallback(
    (password: string) => run(async () => openWith(await vaultCreate(password))),
    [run, openWith],
  );
  const unlock = useCallback(
    (password: string) => run(async () => openWith(await vaultUnlock(password))),
    [run, openWith],
  );
  const lock = useCallback(
    () =>
      run(async () => {
        setStatus(await vaultLock());
        setCredentials([]);
      }),
    [run],
  );
  const addKey = useCallback(
    (input: AddCredentialInput) => run(async () => setCredentials(await vaultAddCredential(input))),
    [run],
  );
  const removeKey = useCallback(
    (exchangeId: string, label: string) =>
      run(async () => setCredentials(await vaultRemoveCredential(exchangeId, label))),
    [run],
  );
  const renewKey = useCallback(
    (input: AddCredentialInput) => run(async () => setCredentials(await vaultReplaceCredential(input))),
    [run],
  );
  const renameKey = useCallback(
    (exchangeId: string, label: string, newLabel: string) =>
      run(async () => setCredentials(await vaultRenameCredential(exchangeId, label, newLabel))),
    [run],
  );
  const changePassword = useCallback(
    (current: string, next: string) => run(async () => setStatus(await vaultChangePassword(current, next))),
    [run],
  );
  const setIdleMinutes = useCallback(
    (minutes: number) => run(async () => setStatus(await vaultSetIdleMinutes(minutes))),
    [run],
  );
  const reset = useCallback(
    () =>
      run(async () => {
        const next = await vaultReset(VAULT_RESET_CONFIRMATION);
        setCredentials([]);
        setStatus(next);
      }),
    [run],
  );

  return {
    status,
    credentials,
    initializing,
    busy,
    error,
    create,
    unlock,
    lock,
    addKey,
    removeKey,
    renewKey,
    renameKey,
    changePassword,
    setIdleMinutes,
    reset,
  };
}
