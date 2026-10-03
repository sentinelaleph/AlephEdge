import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  ANONYMOUS,
  membershipLogin,
  membershipLogout,
  membershipRefresh,
  membershipRestore,
  type MembershipView,
} from "@/lib/ipc/membership/membership";
import { errorMessage } from "@/lib/ipc/bridge";

export interface MembershipController {
  view: MembershipView;
  /** True until the first status read resolves. */
  initializing: boolean;
  /** True while a sign-in/out request is in flight. */
  busy: boolean;
  error: string | null;
  signIn: (email: string, password: string) => Promise<void>;
  signOut: () => Promise<void>;
  /** Re-check billing status (used by the locked gate's retry). */
  refresh: () => Promise<void>;
}

/** Owns membership state: initial status read + sign-in/out actions. */
export function useMembership(): MembershipController {
  const { t } = useTranslation();
  const [view, setView] = useState<MembershipView>(ANONYMOUS);
  const [initializing, setInitializing] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    // Try to silently restore a session from the OS keychain first, so a
    // returning user isn't asked to sign in again on every launch.
    membershipRestore()
      .then((v) => alive && setView(v))
      .catch(() => undefined)
      .finally(() => alive && setInitializing(false));
    return () => {
      alive = false;
    };
  }, []);

  const signIn = useCallback(async (email: string, password: string) => {
    setError(null);
    setBusy(true);
    try {
      setView(await membershipLogin(email, password));
    } catch (e) {
      setError(errorMessage(e, t("membership.loginFailed")));
      throw e;
    } finally {
      setBusy(false);
    }
  }, [t]);

  const signOut = useCallback(async () => {
    setBusy(true);
    try {
      setView(await membershipLogout());
      setError(null);
    } finally {
      setBusy(false);
    }
  }, []);

  const refresh = useCallback(async () => {
    setError(null);
    setBusy(true);
    try {
      setView(await membershipRefresh());
    } catch (e) {
      setError(errorMessage(e, t("membership.refreshFailed")));
    } finally {
      setBusy(false);
    }
  }, [t]);

  return { view, initializing, busy, error, signIn, signOut, refresh };
}
