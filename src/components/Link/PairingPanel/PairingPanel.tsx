import { motion } from "framer-motion";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/Button/Button";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { Panel } from "@/components/ui/Panel/Panel";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import type { LinkStateView, PairingView } from "@/lib/ipc/link/link";
import { SPRING } from "@/lib/motion";

import { LINK_STATUS } from "../linkStatus";
import { QrCode } from "./QrCode";
import { usePairing } from "./usePairing";
import "./PairingPanel.css";
import { localizeError } from "@/lib/errorText";

/**
 * Phone pairing.
 *
 * The QR carries the pairing key, so a code left on screen is a standing
 * credential: anyone who photographs it can stop this desk, start it, or close
 * its positions. Three things follow, and all three are visible here rather
 * than only enforced in Rust:
 *
 *   - a countdown, so the user knows the code is temporary;
 *   - a dead code is REMOVED and replaced with its reason — never left drawn
 *     and looking usable;
 *   - the link state is shown next to it, including "not retrying, and here is
 *     what to do", because a spinner that never resolves teaches people to
 *     ignore the panel.
 */
export function PairingPanel() {
  const { t } = useTranslation();
  const { pairing, link, error, busy, relay, start, cancel, checkRelay } = usePairing();

  return (
    <Panel title={t("link.title")} aside={<LinkBadge link={link} />} className="ae-pair">

      <p className="ae-pair__lede">{t("link.lede")}</p>

      {/* A stopped link carries its OWN message and it says what to do; a
          generic "disconnected" would leave the user waiting for a recovery
          that is never coming. */}
      {link.state === "stopped" ? (
        <p className="ae-pair__error" role="alert">
          {localizeError(link.message)}
        </p>
      ) : null}

      {relay === "checking" ? (
        <p className="ae-pair__checking">{t("link.checking")}</p>
      ) : relay === "down" ? (
        <RelayUnavailable onRetry={() => void checkRelay()} />
      ) : pairing?.qr ? (
        <LiveCode pairing={pairing} onCancel={() => void cancel()} busy={busy} />
      ) : (
        <DeadCode
          reason={pairing?.expiredReason ?? null}
          busy={busy}
          onStart={() => void start()}
        />
      )}

      {error ? (
        <p className="ae-pair__error" role="alert">
          {error}
        </p>
      ) : null}
    </Panel>
  );
}

function RelayUnavailable({ onRetry }: { onRetry: () => void }) {
  const { t } = useTranslation();
  return (
    <EmptyState
      title={t("link.unavailableTitle")}
      detail={t("link.unavailableBody")}
      actions={
        <Button variant="secondary" size="sm" onClick={onRetry}>
          {t("link.checkAgain")}
        </Button>
      }
    />
  );
}

function LiveCode({
  pairing,
  onCancel,
  busy,
}: {
  pairing: PairingView;
  onCancel: () => void;
  busy: boolean;
}) {
  const { t } = useTranslation();
  const secs = Math.ceil(pairing.remainingMs / 1000);
  const frac = pairing.ttlMs > 0 ? pairing.remainingMs / pairing.ttlMs : 0;
  // Under fifteen seconds the code is about to become useless mid-scan; say so
  // before it happens rather than after.
  const urgent = pairing.remainingMs <= 15_000;

  return (
    <motion.div
      className="ae-pair__live"
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      transition={SPRING.snappy}
    >
      <div className="ae-pair__qr">
        {/* Drawn as SVG elements, never as an HTML string — see QrCode.tsx. */}
        <QrCode text={pairing.qr ?? ""} label={t("link.qrAlt")} />
      </div>

      <div className="ae-pair__meta">
        <div className={`ae-pair__count ${urgent ? "is-urgent" : ""}`}>
          <span className="ae-pair__secs">{secs}</span>
          <span className="ae-pair__unit">{t("link.seconds")}</span>
        </div>
        <div className="ae-pair__bar" aria-hidden="true">
          <div className="ae-pair__bar-fill" style={{ transform: `scaleX(${frac})` }} />
        </div>
        <p className="ae-pair__warn">{t("link.singleUse")}</p>
        <Button variant="secondary" size="sm" onClick={onCancel} disabled={busy}>
          {t("link.cancel")}
        </Button>
      </div>
    </motion.div>
  );
}

function DeadCode({
  reason,
  busy,
  onStart,
}: {
  reason: string | null;
  busy: boolean;
  onStart: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="ae-pair__dead">
      {/* A dead code is not redrawn faintly or greyed out: it is gone, and its
          reason takes its place. */}
      {reason ? <p className="ae-pair__reason">{localizeError(reason)}</p> : null}
      <Button size="sm" onClick={onStart} disabled={busy}>
        {reason ? t("link.newCode") : t("link.start")}
      </Button>
    </div>
  );
}

function LinkBadge({ link }: { link: LinkStateView }) {
  const { t } = useTranslation();
  return (
    <div className="ae-pair__status">
      {link.state === "retrying" ? (
        <span className="ae-pair__hint tabular">{t("link.retryIn", { secs: link.in_secs })}</span>
      ) : null}
      <StatusChip status={LINK_STATUS[link.state]} label={t(`link.state.${link.state}`)} />
    </div>
  );
}
