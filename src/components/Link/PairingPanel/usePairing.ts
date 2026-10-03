import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  beginPairing,
  cancelPairing,
  linkState,
  pairingStatus,
  relayHealth,
  type LinkStateView,
  type PairingView,
} from "@/lib/ipc/link/link";
import { errorMessage } from "@/lib/ipc/bridge";

export type RelayHealth = "checking" | "up" | "down";

const RELAY_POLL_MS = 30_000;

/**
 * Pairing + link state, polled.
 *
 * Polls once a second because the QR is a COUNTDOWN: a code that quietly
 * expires while still drawn on screen would be a control the user believes in
 * and cannot use. The countdown is the honest part of this panel.
 *
 * `relay` tracks whether the pairing relay itself is reachable. The relay is
 * not deployed and the phone app has not shipped, so a working-looking QR
 * and countdown would be a lie — the panel must know this before it draws
 * anything.
 */
export function usePairing() {
  const { t } = useTranslation();
  const [pairing, setPairing] = useState<PairingView | null>(null);
  const [link, setLink] = useState<LinkStateView>({ state: "unpaired" });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [relay, setRelay] = useState<RelayHealth>("checking");
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  const checkRelay = useCallback(async () => {
    setRelay("checking");
    try {
      const ok = await relayHealth();
      if (!alive.current) return;
      setRelay(ok ? "up" : "down");
    } catch {
      if (!alive.current) return;
      setRelay("down");
    }
  }, []);

  useEffect(() => {
    void checkRelay();
    const t = setInterval(() => void checkRelay(), RELAY_POLL_MS);
    return () => clearInterval(t);
  }, [checkRelay]);

  const refresh = useCallback(async () => {
    try {
      const [p, l] = await Promise.all([pairingStatus(), linkState()]);
      if (!alive.current) return;
      setPairing(p);
      setLink(l);
    } catch {
      /* keep the last good view; a transient poll failure is not news */
    }
  }, []);

  useEffect(() => {
    void refresh();
    const t = setInterval(() => void refresh(), 1000);
    return () => clearInterval(t);
  }, [refresh]);

  const start = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      // No host and no desk id pass through here. Rust resolves both and starts
      // the relay session itself; this used to fetch them and hand them back
      // over IPC, which made a security boundary depend on a UI convention.
      setPairing(await beginPairing());
    } catch (e) {
      setError(errorMessage(e, t("link.error")));
    } finally {
      setBusy(false);
    }
  }, [t]);

  const cancel = useCallback(async () => {
    setBusy(true);
    try {
      await cancelPairing();
      setPairing(null);
    } finally {
      setBusy(false);
    }
  }, []);

  return { pairing, link, error, busy, relay, start, cancel, checkRelay };
}
