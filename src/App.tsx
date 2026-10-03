import { useEffect, useRef } from "react";
import { recordVaultAutoLock } from "@/app/alerts";
import { useMembership } from "@/components/Membership/useMembership";
import { Aurora } from "@/components/Shell/Aurora/Aurora";
import { TopBar } from "@/components/Shell/TopBar";
import { useVault } from "@/components/Vault/useVault";
import { isDeskOpen, Workspace } from "@/components/Workspace/Workspace";
import { onVaultAutoLocked } from "@/lib/ipc/vault/vault";
import { signalDisconnect } from "@/lib/ipc/signal/signal";
import "./App.css";

/**
 * Composition root. Session state (membership + vault) is owned here. Until
 * every gate has passed, the gate screens sit on the Aurora backdrop under the
 * gate bar (brand, language, theme); after that the desk takes the whole
 * window with its own sidebar, header and status bar.
 */
export default function App() {
  const membership = useMembership();
  const vault = useVault();
  const open = isDeskOpen(membership, vault);

  // Signing out ends the right to signals: tear the stream down. (It is
  // started again by the desk on the next sign-in.)
  const wasSignedIn = useRef(false);
  useEffect(() => {
    const signedIn = membership.view.authenticated;
    if (wasSignedIn.current && !signedIn) void signalDisconnect().catch(() => undefined);
    wasSignedIn.current = signedIn;
  }, [membership.view.authenticated]);

  // The gate takes over on auto-lock; remember when, for the alerts list.
  useEffect(() => {
    let alive = true;
    const pending = onVaultAutoLocked(() => {
      if (alive) recordVaultAutoLock(Date.now());
    });
    return () => {
      alive = false;
      void pending.then((unlisten) => unlisten());
    };
  }, []);

  if (open) {
    return <Workspace membership={membership} vault={vault} />;
  }

  // The exchange the gate bar's cockpit check probes — Binance, once vaulted.
  const cockpitExchange = vault.credentials.some((c) => c.exchangeId === "binance") ? "binance" : null;

  return (
    <div className="ae-app">
      <Aurora />
      <TopBar membership={membership} cockpitExchange={cockpitExchange} />
      <main className="ae-stage">
        <Workspace membership={membership} vault={vault} />
      </main>
    </div>
  );
}
