import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import type { MembershipController } from "../useMembership";
import "./AccountChip.css";

interface AccountChipProps {
  membership: MembershipController;
}

/**
 * Topbar account presence: who is signed in and a way out. Hidden entirely when
 * signed out (the workspace shows the login form instead).
 */
export function AccountChip({ membership }: AccountChipProps) {
  const { t } = useTranslation();
  const { view, busy, signOut } = membership;

  if (!view.authenticated) return null;

  return (
    <div className="ae-account">
      <span className="ae-account__dot" data-state={view.state} aria-hidden="true" />
      <span className="ae-account__name">{view.displayName ?? view.email}</span>
      <Button variant="ghost" size="sm" onClick={signOut} disabled={busy}>
        {t("membership.signOut")}
      </Button>
    </div>
  );
}
