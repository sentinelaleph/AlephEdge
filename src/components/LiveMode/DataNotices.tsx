import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { localizeError } from "@/lib/errorText";

/**
 * App-wide: no bot may start because the risk settings or the store could
 * not be read (Rust `app::data_health`). Says which, and where it is fixed.
 */
export function TradingBlockedBanner() {
  const { t } = useTranslation();
  const { desk } = useDeskContext();
  const code = desk.loaded ? (desk.status.tradingBlocked ?? null) : null;
  if (!code) return null;
  const risk = code === "riskConfigUnreadable";
  return (
    <div className="ae-banner" data-tone="danger" role="alert">
      <span>
        {t("liveMode.blocked.head")} {localizeError(code)}
      </span>
      {risk ? (
        <Link to="/risk?s=level" className="ae-link">
          {t("nav.risk")}
        </Link>
      ) : null}
    </div>
  );
}

/** Risk page: the settings file could not be read; these are the defaults until saved. */
export function RiskConfigNotice() {
  const { t } = useTranslation();
  const { risk } = useDeskContext();
  const kept = risk.state?.configUnreadable;
  if (kept === undefined || kept === null) return null;
  const level = risk.state?.limits.level;
  return (
    <div className="ae-banner" data-tone="danger" role="alert">
      <span>{t(kept ? "liveMode.riskUnreadable.kept" : "liveMode.riskUnreadable.notKept", { file: kept })}</span>
      <Button size="xs" disabled={risk.busy || !level} onClick={() => level && void risk.selectLevel(level)}>
        {t("liveMode.riskUnreadable.keep")}
      </Button>
    </div>
  );
}

/**
 * Real-money position rows the book does not manage: unreadable, or on a
 * Paper bot. Money in play the desk cannot see the size of: close it on the
 * exchange or restart with a build that reads it.
 */
export function LiveUnreadableNotice() {
  const { t } = useTranslation();
  const { desk } = useDeskContext();
  const rows = desk.loaded ? (desk.status.liveUnreadable ?? []) : [];
  if (rows.length === 0) return null;
  return (
    <div className="ae-banner" data-tone="danger" role="alert">
      <span>
        {t("liveMode.heldRows", { count: rows.length })} <span className="mono">{rows.join(", ")}</span>
      </span>
      <Link to="/positions?tab=exchange" className="ae-link">
        {t("strategy.live.goExchange")}
      </Link>
    </div>
  );
}
