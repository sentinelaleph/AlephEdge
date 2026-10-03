import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/Button/Button";
import { enter } from "@/lib/motion";
import type { MembershipView } from "@/lib/ipc/membership/membership";
import "./MembershipGate.css";

interface MembershipGateProps {
  view: MembershipView;
  busy: boolean;
  onRetry: () => void;
  onSignOut: () => void;
}

/**
 * Shown when the user is signed in but the bot is not permitted to trade —
 * membership inactive, or its status could not be verified. The bot stays
 * locked in both cases (fail-safe): there is no "assume active" path.
 */
export function MembershipGate({ view, busy, onRetry, onSignOut }: MembershipGateProps) {
  const { t } = useTranslation();
  const unknown = view.state === "unknown";

  return (
    <motion.section className="ae-gate" {...enter}>
      <span className="ae-gate__badge" data-state={view.state}>
        {t(`membership.state.${view.state}`)}
      </span>
      <h1 className="ae-gate__title">{t("membership.lockedTitle")}</h1>
      <p className="ae-gate__body">
        {unknown ? t("membership.lockedUnknown") : t("membership.lockedInactive")}
      </p>
      {view.email ? (
        <p className="ae-gate__meta">
          {t("membership.signedInAs", { email: view.email })}
        </p>
      ) : null}

      <div className="ae-gate__actions">
        {unknown ? (
          <Button onClick={onRetry} disabled={busy}>
            {t("membership.retry")}
          </Button>
        ) : null}
        <Button variant="secondary" onClick={onSignOut} disabled={busy}>
          {t("membership.signOut")}
        </Button>
      </div>
    </motion.section>
  );
}
