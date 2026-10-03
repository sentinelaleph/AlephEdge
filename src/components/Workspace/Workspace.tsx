import { useTranslation } from "react-i18next";
import { AppShell } from "@/app/AppShell/AppShell";
import { DeskProvider } from "@/app/DeskProvider";
import { LoginForm } from "@/components/Membership/LoginForm/LoginForm";
import { MembershipGate } from "@/components/Membership/MembershipGate/MembershipGate";
import type { MembershipController } from "@/components/Membership/useMembership";
import { VaultLock } from "@/components/Vault/VaultLock/VaultLock";
import type { VaultController } from "@/components/Vault/useVault";
import "./Workspace.css";

interface WorkspaceProps {
  membership: MembershipController;
  vault: VaultController;
}

/**
 * The single-stage flow gate (PRD: Open → verify membership → unlock vault →
 * desk). Each guard fails safe: the vault surface is unreachable until
 * membership is active, and the desk (router, pages, bots) is unreachable until
 * the vault is unlocked. The gates never read the URL hash; a deep link is
 * replayed by the router once the desk mounts.
 */
export function isDeskOpen(membership: MembershipController, vault: VaultController): boolean {
  return (
    !membership.initializing &&
    !vault.initializing &&
    membership.view.authenticated &&
    membership.view.active &&
    vault.status.state === "unlocked"
  );
}

/** Gates in fixed order: loading → sign-in → membership → vault create/unlock → desk. */
export function Workspace({ membership, vault }: WorkspaceProps) {
  const { t } = useTranslation();

  if (membership.initializing || vault.initializing) {
    return <p className="ae-workspace__loading">{t("workspace.loading")}</p>;
  }

  if (!membership.view.authenticated) {
    return (
      <LoginForm onSignIn={membership.signIn} busy={membership.busy} error={membership.error} />
    );
  }

  if (!membership.view.active) {
    return (
      <MembershipGate
        view={membership.view}
        busy={membership.busy}
        onRetry={membership.refresh}
        onSignOut={membership.signOut}
      />
    );
  }

  if (vault.status.state === "absent") {
    return (
      <VaultLock mode="create" busy={vault.busy} error={vault.error} onSubmit={vault.create} />
    );
  }

  if (vault.status.state === "locked") {
    return (
      <VaultLock
        mode="unlock"
        busy={vault.busy}
        error={vault.error}
        onSubmit={vault.unlock}
        onReset={vault.reset}
      />
    );
  }

  // Past every gate: the desk state lives above the router, so moving between
  // pages never restarts polling or the signal stream.
  return (
    <DeskProvider membership={membership} vault={vault}>
      <AppShell />
    </DeskProvider>
  );
}
