import { useRef } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate, useRouter } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { DeskFeed } from "@/components/BotDesk/DeskFeed/DeskFeed";
import { DeskPrinciples } from "@/components/BotDesk/DeskPrinciples/DeskPrinciples";
import { RegimeCard, RiskReadout, StreamCard } from "@/components/Desk/Readouts";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip, UntestedChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { FilterGroup, listParam } from "@/components/ui/FilterPanel/FilterPanel";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatUsdt, NO_VALUE } from "@/lib/format";
import type { BotKind } from "@/lib/ipc/bot/bot";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { groupSkips, skipParams } from "@/lib/skipNotes";
import { signalBotRows, type BotRow } from "./botRows";
import { BotStateChip, SignalRowAction } from "./BotsTable";
import { SignalBotForm } from "./signal/SignalBotForm";
import "./bots.css";
import { MarketTrendPanel } from "@/components/Desk/MarketTrend";

const KINDS: BotKind[] = ["futures", "spot", "pump"];

function utcDayStart(): number {
  const d = new Date();
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
}

/**
 * #/bots/signal: the three signal bots (Futures / Spot / Pump) as one table,
 * the selected bot's settings as a form (?bot=), and the desk feed. Same IPC
 * as before: bot_configure + bot_start, bot_stop, bot_set_live (typed LIVE,
 * Futures, live builds only).
 */
export function SignalBotsPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const ctx = useDeskContext();
  const { desk, risk, catalog, feed, pnl } = ctx;
  const { route } = useRouter();
  const kindFilter = listParam(route.query.get("kind")) as BotKind[];
  const reasonFilter = listParam(route.query.get("reason"));
  const botParam = route.query.get("bot") as BotKind | null;
  const selected: BotKind = botParam && KINDS.includes(botParam) ? botParam : "futures";
  const settingsRef = useRef<HTMLDivElement>(null);
  const s = desk.status;
  const maxCapitalQuote = risk.state?.maxCapitalQuote ?? null;

  const setQuery = (patch: { kind?: string[]; reason?: string[]; bot?: BotKind }) =>
    navigate(
      withQuery("/bots/signal", {
        kind: (patch.kind ?? kindFilter).join(","),
        reason: (patch.reason ?? reasonFilter).join(","),
        bot: (patch.bot ?? selected) === "futures" ? "" : (patch.bot ?? selected),
      }),
      { replace: true },
    );

  const rows = desk.loaded ? signalBotRows(s, maxCapitalQuote) : [];
  const row = rows.find((r) => r.kind === selected) ?? null;
  // The feed filters act on what the feed shows; the table keeps the real status.
  const positions = kindFilter.length ? s.openPositions.filter((p) => kindFilter.includes(p.botKind)) : s.openPositions;
  const skips = reasonFilter.length ? s.recentSkips.filter((n) => reasonFilter.includes(n.reason)) : s.recentSkips;
  const groups = groupSkips(s.recentSkips);
  const anyLive = KINDS.some((k) => s[k]?.live === true);
  const today = utcDayStart();
  const maxLeverage = risk.state?.limits.maxLeverage ?? null;
  const exchanges = catalog.exchanges;
  const formReady = desk.loaded && maxLeverage !== null && exchanges !== null;

  const openSettings = (kind: BotKind) => {
    setQuery({ bot: kind });
    requestAnimationFrame(() => settingsRef.current?.scrollIntoView({ block: "start", behavior: "smooth" }));
  };

  const columns: DataColumn<BotRow>[] = [
    {
      id: "name",
      header: t("table.name"),
      cell: (r) => (
        <span className="ae-namecell">
          <Link to={`/bots/${r.id}`} className="ae-link" title={r.kind === "pump" ? t("bots.pumpNote") : undefined}>
            {t(`bots.kind.${r.kind}`)}
          </Link>
          {r.untested ? <UntestedChip title={t("bots.pumpNote")} /> : null}
          {r.live ? <LiveChip /> : null}
        </span>
      ),
    },
    { id: "market", header: t("table.market"), priority: 2, cell: (r) => t(`filters.market.${r.market}`) },
    {
      id: "exchange",
      header: t("table.exchange"),
      priority: 3,
      cell: (r) => (r.exchangeId ? exchangeName(r.exchangeId, catalog.exchanges) : NO_VALUE),
    },
    { id: "state", header: t("table.state"), cell: (r) => <BotStateChip row={r} /> },
    { id: "open", header: t("table.openPositions"), numeric: true, priority: 2, cell: (r) => r.openPositions },
    {
      id: "capital",
      header: t("table.capital"),
      numeric: true,
      priority: 2,
      cell: (r) => (r.capital !== null ? formatUsdt(r.capital, locale) : NO_VALUE),
    },
    { id: "max", header: t("table.maxPositions"), numeric: true, priority: 3, cell: (r) => r.maxPositions ?? NO_VALUE },
  ];

  const left = (
    <>
      <RiskReadout />
      <RegimeCard />
      <StreamCard />
      <FilterGroup
        label={t("signalBots.feedKind")}
        options={KINDS.map((k) => ({ value: k, label: t(`bots.kind.${k}`), count: s.openPositions.filter((p) => p.botKind === k).length }))}
        selected={kindFilter}
        onChange={(v) => setQuery({ kind: v })}
      />
      {groups.length > 0 ? (
        <FilterGroup
          label={t("signalBots.feedReason")}
          options={groups.map((g) => ({ value: g.reason, label: t(`bots.skipReasons.${g.reason}`, skipParams(g.latest)), count: g.count }))}
          selected={reasonFilter}
          onChange={(v) => setQuery({ reason: v })}
        />
      ) : null}
    </>
  );

  const right = (
    <>
      <MarketTrendPanel />
      <Panel title={t("signalBots.todayTitle")}>
        <FactList
          rows={KINDS.flatMap((k) => [
            {
              label: t("signalBots.openOf", { kind: t(`bots.kind.${k}`) }),
              value: s.openPositions.filter((p) => p.botKind === k).length,
            },
            {
              label: t("signalBots.closedTodayOf", { kind: t(`bots.kind.${k}`) }),
              value: pnl.trades.filter((tr) => tr.botKind === k && tr.closedAt >= today).length,
            },
          ])}
        />
      </Panel>
      <Panel title={t("signalBots.skipSummary")} aside={<span className="ae-subtle tabular">{s.recentSkips.length}</span>}>
        {groups.length === 0 ? (
          <p className="ae-subtle">{t("bots.noSkips")}</p>
        ) : (
          <ul className="ae-list">
            {groups.map((g) => (
              <li key={g.reason} title={`${g.latest.symbol}: ${t(`bots.skipReasons.${g.reason}`, skipParams(g.latest))}`}>
                <span>{t(`bots.skipReasons.${g.reason}`, skipParams(g.latest))}</span>
                <span className="tabular">{g.count}</span>
              </li>
            ))}
          </ul>
        )}
      </Panel>
      <DeskPrinciples />
    </>
  );

  return (
    <PageShell title={t("nav.signalBots")} crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]} left={left} right={right}>
      {feed.loaded && !feed.health.connected ? (
        <div className="ae-banner" data-tone="warn" role="status">
          <span>{t("signalBots.streamDown")}</span>
          <Button variant="secondary" size="sm" onClick={() => void feed.reconnect()}>
            {t("statusbar.reconnect")}
          </Button>
        </div>
      ) : null}
      {anyLive ? (
        <p className="ae-banner" data-tone="live">
          {t("signalBots.liveActive")}
        </p>
      ) : null}
      {s.killSwitchTripped ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {t("bots.killSwitch")}
        </p>
      ) : null}
      {/* Sentinel's BTC-break declaration pauses new LONG entries only; shown
          so the desk never looks mysteriously idle during a breakdown. */}
      {s.btcRegime === "break" ? (
        <p className="ae-banner" data-tone="warn">
          {t("bots.btcBreak")}
        </p>
      ) : null}
      {/* The risk level or balance changed under a running bot: its capital
          is now above the cap and every signal is skipped. One line per bot,
          not one skip per signal. */}
      {maxCapitalQuote !== null
        ? rows
            .filter((r) => r.running && r.capitalAboveCap && r.capital !== null)
            .map((r) => (
              <p key={`cap-${r.kind}`} className="ae-banner" data-tone="warn" role="status">
                {t("signalBots.capitalAboveCap", {
                  kind: t(`bots.kind.${r.kind}`),
                  capital: formatUsdt(r.capital ?? 0, locale),
                  cap: formatUsdt(maxCapitalQuote, locale),
                })}
              </p>
            ))
        : null}
      {desk.error ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {desk.error}
        </p>
      ) : null}
      {catalog.error ? (
        <div className="ae-banner" data-tone="danger" role="alert">
          <span>{catalog.error}</span>
          <Button variant="secondary" size="sm" onClick={catalog.retry}>
            {t("common.retry")}
          </Button>
        </div>
      ) : null}

      <DataTable
        label={t("nav.signalBots")}
        columns={columns}
        rows={rows}
        rowKey={(r) => r.id}
        loading={!desk.loaded}
        isSelected={(r) => r.kind === selected}
        actions={(r) => (
          <>
            <SignalRowAction row={r} />
            <Button variant="ghost" size="xs" onClick={() => openSettings(r.kind)}>
              {t("botDetail.tabs.settings")}
            </Button>
          </>
        )}
      />

      {desk.loaded ? <DeskFeed positions={positions} skips={skips} /> : null}

      <div ref={settingsRef} className="ae-scrollanchor">
        <Section title={t("botDetail.tabs.settings")} count={t(`bots.kind.${selected}`)}>
          {formReady && row ? (
            <SignalBotForm
              key={selected}
              kind={selected}
              config={row.config}
              running={row.running}
              busy={desk.busy}
              disabled={s.killSwitchTripped}
              exchangeLocked={row.live || s.openPositions.some((p) => p.botKind === selected && p.live)}
              disabledReasonKey={row.startBlockKey}
              maxLeverage={selected === "spot" ? 1 : maxLeverage}
              maxCapitalQuote={maxCapitalQuote}
              levelMaxPositions={risk.state?.limits.maxConcurrentPositions ?? null}
              exchanges={exchanges}
              live={
                selected === "futures" && s.liveTradingEnabled
                  ? {
                      binanceIsProduction: s.binanceIsProduction,
                      onSetLive: (enabled, confirmation) => desk.setLive("futures", enabled, confirmation),
                      pilotLeft: desk.status.pilotLeft ?? 0,
                      onEndPilot: () => desk.endPilot("futures"),
                      liveVenues: s.liveVenues,
                      venueSandbox: s.venueSandbox,
                    }
                  : undefined
              }
              onStart={(c) => void desk.configureAndStart(c)}
              onStop={() => void desk.stop(selected)}
            />
          ) : catalog.error ? null : (
            <p className="ae-subtle">{t("workspace.loading")}</p>
          )}
        </Section>
      </div>
    </PageShell>
  );
}
