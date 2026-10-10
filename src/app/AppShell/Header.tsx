import { useTranslation } from "react-i18next";
import { MarketTrendChip } from "@/components/Desk/MarketTrend";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { Tooltip } from "@/components/ui/Tooltip/Tooltip";
import { Cockpit, type CockpitReadings } from "@/components/Shell/Cockpit/Cockpit";
import { streamLabelKey, streamTone } from "@/lib/ipc/signal/streamStatus";
import { useStrategyLive } from "@/pages/Bots/strategy/useStrategyLive";
import { Icon } from "./icons";

interface HeaderProps {
  /** Drawer mode: show the hamburger that opens the sidebar. */
  showMenu: boolean;
  menuOpen: boolean;
  onMenu: () => void;
  headingRef: (el: HTMLDivElement | null) => void;
  actionsRef: (el: HTMLDivElement | null) => void;
  unread: number;
  alertsOpen: boolean;
  onAlerts: () => void;
}

/**
 * The global header: the page heading and actions (filled by the page through
 * portals), then global state. The scope indicator is a readout, never a
 * switch: real money is enabled only per bot, by typing LIVE.
 *
 * PageHeader contract (see PageShell): crumb + title left; page actions
 * right as [secondary…][one primary][panel toggle]; then the global zone
 * [mode chip][testnet][demo venues][kill switch][alerts][Exchange pill][Sentinel pill]
 * [account]. The mode chip (Paper / Paper + LIVE) lives HERE ONLY: pages do
 * not repeat a page-level "Paper" / "Simulated fills" chip.
 */
export function Header({ showMenu, menuOpen, onMenu, headingRef, actionsRef, unread, alertsOpen, onAlerts }: HeaderProps) {
  const { t } = useTranslation();
  const { desk, endpoints, membership, accountExchange, feed, health } = useDeskContext();
  const s = desk.status;
  const strategyLive = useStrategyLive().anyLive;
  const liveScope =
    desk.loaded && s.liveTradingEnabled && ([s.futures, s.spot, s.pump].some((c) => c?.live === true) || strategyLive);
  const email = membership.view.email ?? membership.view.displayName ?? "";
  const tone = streamTone(feed.health, feed.loaded);
  const exHealth = accountExchange ? health?.exchanges.find((e) => e.id === accountExchange) : undefined;
  const readings: CockpitReadings = {
    sentinel: {
      state: tone === "success" ? "ok" : tone === "warn" ? "warn" : tone === "danger" ? "down" : "idle",
      label: feed.loaded ? t(streamLabelKey(feed.health)) : t("statusbar.connecting"),
      latencyMs: feed.health.latencyMs ?? null,
      lastSignalSecs: feed.health.lastSignalSecs ?? null,
    },
    exchange: exHealth ? { level: exHealth.level, latencyMs: exHealth.latencyMs } : null,
  };

  return (
    <header className="ae-header">
      {showMenu ? (
        <button
          type="button"
          className="ae-header__icon"
          aria-label={t("nav.openMenu")}
          aria-expanded={menuOpen}
          onClick={onMenu}
        >
          <Icon name="menu" />
        </button>
      ) : null}
      <div className="ae-header__heading" ref={headingRef} />
      <div className="ae-header__actions" ref={actionsRef} />

      <div className="ae-header__global">
        {desk.loaded ? (
          <span
            className="ae-header__scope"
            data-live={liveScope || undefined}
            title={t(liveScope ? "nav.scopeLiveTooltip" : "nav.scopePaperTooltip")}
          >
            {liveScope ? t("nav.scopePaperLive") : t("states.paper")}
          </span>
        ) : null}
        {endpoints && !endpoints.binanceIsProduction ? (
          <span className="ae-header__testnet" title={t("app.testnetTooltip", { host: endpoints.binanceFuturesBase })}>
            {t("app.testnetBadge")}
          </span>
        ) : null}
        {desk.loaded && s.venueSandbox ? (
          <span className="ae-header__testnet" title={t("app.sandboxTooltip")}>
            {t("app.sandboxBadge")}
          </span>
        ) : null}
        <MarketTrendChip />
        {desk.loaded && s.killSwitchTripped ? (
          <Link to="/risk" className="ae-header__kill">
            {t("nav.killSwitchTripped")}
          </Link>
        ) : null}
        <Tooltip content={t("alerts.open", { count: unread })} placement="bottom">
          <button
            type="button"
            className="ae-header__icon ae-header__bell"
            aria-label={t("alerts.open", { count: unread })}
            aria-expanded={alertsOpen}
            onClick={onAlerts}
          >
            <Icon name="bell" />
            {unread > 0 ? <span className="ae-header__unread tabular">{unread}</span> : null}
          </button>
        </Tooltip>
        <Cockpit exchangeId={accountExchange} readings={readings} />
        <Link to="/account" className="ae-header__account" aria-label={`${t("nav.account")} · ${email}`} title={email}>
          <span className="ae-header__avatar" aria-hidden="true">
            {(email || "?").slice(0, 1).toUpperCase()}
          </span>
          <span className="ae-header__email">{email}</span>
        </Link>
      </div>
    </header>
  );
}
