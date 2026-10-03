import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { deriveAlerts, useAlertPrefs, useNow, useVaultAutoLockedAt } from "@/app/alerts";
import { AlertsList } from "@/app/AppShell/AlertsDrawer";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate } from "@/app/router/router";
import { useAccount } from "@/components/Account/useAccount";
import { KillSwitchCard, RegimeCard, StreamCard } from "@/components/Desk/Readouts";
import { FirstRunChecklist } from "@/components/Shell/FirstRunChecklist/FirstRunChecklist";
import { AnimatedNumber } from "@/components/ui/AnimatedNumber/AnimatedNumber";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile, Meter } from "@/components/ui/KpiTile/KpiTile";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import {
  formatNumber,
  formatPercent,
  formatPnl,
  formatSignedPercent,
  formatTableTime,
  formatUsdt,
  NO_VALUE,
  pnlToneAttr,
} from "@/lib/format";
import type { TradeRecord } from "@/lib/ipc/trades/trades";
import { isActive, type StrategyBotView, type StrategyKind } from "@/lib/ipc/strategy/strategy";
import { runStateText, sideText } from "@/lib/strategyText";
import { signalBotRows, type BotRow } from "@/pages/Bots/botRows";
import { BotStateChip, SignalRowAction } from "@/pages/Bots/BotsTable";
import { runStatusKind, totalPnlPct } from "@/pages/Bots/strategy/StrategyBotsTable";
import { useStrategyActions } from "@/pages/Bots/strategy/useStrategyActions";
import "@/pages/Bots/bots.css";

/** One row of the dashboard's bot table: a signal bot or a DCA / Grid bot. */
interface DashBotRow {
  id: string;
  signal?: BotRow;
  bot?: StrategyBotView;
}

const CHECKLIST_KEY = "aleph-edge-setup-dismissed";

function readDismissed(): boolean {
  try {
    return localStorage.getItem(CHECKLIST_KEY) === "1";
  } catch {
    return false;
  }
}

/**
 * #/dashboard: what is running, what it made or lost, what needs attention.
 * Only numbers the desk reports are shown; a figure the backend does not
 * report yet (floating P&L, equity series) is absent, never estimated.
 */
export function DashboardPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const ctx = useDeskContext();
  const { desk, risk, pnl, feed, accountExchange, strategy } = ctx;
  const strategyActions = useStrategyActions();
  const [dismissed, setDismissed] = useState(readDismissed);
  const now = useNow();
  const lockedAt = useVaultAutoLockedAt();
  const { prefs } = useAlertPrefs();
  const alerts = useMemo(
    () => deriveAlerts(ctx, t, lockedAt, now).filter((a) => prefs[a.category]),
    [ctx, t, lockedAt, now, prefs],
  );

  const s = desk.status;
  const rows = desk.loaded ? signalBotRows(s, risk.state?.allowsPump ?? false) : [];
  const configured = rows.filter((r) => r.config);
  const running = rows.filter((r) => r.running);
  const liveBots = rows.filter((r) => r.live);
  const stats = pnl.stats;
  const balance = risk.state?.balance ?? null;
  const limit = risk.state ? (risk.state.balance * risk.state.effectiveDailyLossPct) / 100 : null;
  const today = stats?.todayPnlQuote ?? null;
  const capitalInUse = s.openPositions.reduce((n, p) => n + p.capital, 0);
  // DCA / Grid bots (paper only); null until the first strategy list arrives.
  const strategyBots = strategy.bots;
  const strategyActive = strategyBots ? strategyBots.filter(isActive).length : 0;
  const strategyCycles = strategyBots ? strategyBots.filter((b) => b.openCycle !== null).length : 0;
  const strategyCount = (kind: StrategyKind) => {
    if (!strategyBots) return NO_VALUE;
    const of = strategyBots.filter((b) => b.kind === kind);
    return t("dashboard.runningOf", { running: of.filter(isActive).length, total: of.length });
  };
  const usdt = (n: number) => formatPnl(n, locale).text;
  const decided = stats ? stats.wins + stats.losses : 0;

  const botRows: DashBotRow[] = [
    ...configured.map((r) => ({ id: r.id, signal: r })),
    ...(strategyBots ?? []).map((b) => ({ id: b.id, bot: b })),
  ];
  const botColumns: DataColumn<DashBotRow>[] = [
    {
      id: "name",
      header: t("table.name"),
      cell: (r) =>
        r.signal ? (
          <span className="ae-namecell">
            <Link to={`/bots/${r.id}`} className="ae-link">
              {t(`bots.kind.${r.signal.kind}`)}
            </Link>
            {r.signal.live ? <LiveChip /> : null}
          </span>
        ) : (
          <Link to={`/bots/${r.id}`} className="ae-link">
            {r.bot?.name}
          </Link>
        ),
    },
    {
      id: "type",
      header: t("table.type"),
      priority: 2,
      cell: (r) => t(`botsList.type.${r.signal ? "signal" : r.bot?.kind}`),
    },
    {
      id: "market",
      header: t("table.market"),
      priority: 3,
      cell: (r) =>
        r.signal
          ? t(`filters.market.${r.signal.market}`)
          : r.bot
            ? `${t(`filters.market.${r.bot.market}`)} · ${sideText(t, r.bot.side)}${r.bot.leverage > 1 ? ` · ${r.bot.leverage}x` : ""}`
            : NO_VALUE,
    },
    {
      id: "pairs",
      header: t("table.pairs"),
      priority: 2,
      cell: (r) =>
        r.signal ? (r.signal.symbols.length === 0 ? t("botsList.allPairs") : r.signal.symbols.join(", ")) : r.bot?.symbol,
    },
    {
      id: "state",
      header: t("table.state"),
      cell: (r) =>
        r.signal ? (
          <BotStateChip row={r.signal} />
        ) : r.bot ? (
          <StatusChip status={runStatusKind(r.bot)} label={runStateText(t, r.bot)} />
        ) : null,
    },
    {
      id: "open",
      header: t("table.openPositions"),
      numeric: true,
      priority: 2,
      cell: (r) => (r.signal ? r.signal.openPositions : r.bot?.openCycle ? 1 : 0),
    },
    {
      id: "pnl",
      header: t("strategy.col.totalPnl"),
      numeric: true,
      priority: 3,
      cell: (r) => (r.bot ? formatSignedPercent(totalPnlPct(r.bot), locale) : NO_VALUE),
      tone: (r) => (r.bot ? pnlToneAttr(totalPnlPct(r.bot)) : "muted"),
    },
  ];
  const tradeColumns: DataColumn<TradeRecord>[] = [
    { id: "closed", header: t("table.closedAt"), cell: (tr) => formatTableTime(tr.closedAt, locale) },
    {
      id: "bot",
      header: t("table.bot"),
      priority: 2,
      cell: (tr) => (
        <span className="ae-namecell">
          {t(`bots.kind.${tr.botKind}`, { defaultValue: tr.botKind })}
          {tr.live ? <LiveChip /> : null}
        </span>
      ),
    },
    { id: "symbol", header: t("table.symbol"), cell: (tr) => tr.symbol },
    {
      id: "side",
      header: t("table.side"),
      priority: 2,
      cell: (tr) => t(`signalDesk.direction.${tr.direction === "short" ? "short" : "long"}`),
    },
    {
      id: "pnlPct",
      header: t("table.pnlPct"),
      numeric: true,
      cell: (tr) => formatSignedPercent(tr.pnlPct, locale),
      tone: (tr) => pnlToneAttr(tr.pnlPct),
    },
    {
      id: "pnlUsdt",
      header: t("table.pnlUsdt"),
      numeric: true,
      cell: (tr) => formatPnl(tr.pnlUsdt, locale, { unit: "none" }).text,
      tone: (tr) => pnlToneAttr(tr.pnlUsdt),
    },
    {
      id: "exit",
      header: t("table.exitReason"),
      priority: 3,
      cell: (tr) => t(`pnl.exit.${tr.exitReason}`, { defaultValue: tr.exitReason }),
    },
  ];
  const scopeNote = pnl.split ? t(`pnl.scope.${pnl.scope}`) : t("states.paper");

  const dismiss = () => {
    setDismissed(true);
    try {
      localStorage.setItem(CHECKLIST_KEY, "1");
    } catch {
      /* not remembered */
    }
  };
  const allDone = feed.health.connected && configured.length > 0 && running.length > 0;

  const left = (
    <>
      <Panel title={t("dashboard.scope")}>
        <FactList
          rows={[
            { label: t("dashboard.paperRunning"), value: running.filter((r) => !r.live).length + strategyActive },
            { label: t("dashboard.liveRunning"), value: running.filter((r) => r.live).length, tone: liveBots.length ? "danger" : undefined },
          ]}
        />
      </Panel>
      <Panel title={t("nav.groups.bots")}>
        <ul className="ae-list">
          <li>
            <Link to="/bots/signal" className="ae-link">
              {t("nav.signalBots")}
            </Link>
            <span className="tabular">{t("dashboard.runningOf", { running: running.length, total: configured.length })}</span>
          </li>
          <li>
            <Link to="/bots/dca" className="ae-link">
              {t("nav.dcaBots")}
            </Link>
            <span className="tabular">{strategyCount("dca")}</span>
          </li>
          <li>
            <Link to="/bots/grid" className="ae-link">
              {t("nav.gridBots")}
            </Link>
            <span className="tabular">{strategyCount("grid")}</span>
          </li>
        </ul>
      </Panel>
      <RegimeCard />
      <StreamCard />
    </>
  );

  const right = (
    <>
      <Panel title={t("alerts.title")} aside={<span className="ae-subtle tabular">{alerts.length}</span>}>
        <AlertsList alerts={alerts} />
      </Panel>
      <KillSwitchCard />
      {accountExchange ? <AccountSummary exchangeId={accountExchange} /> : null}
    </>
  );

  return (
    <PageShell
      title={t("nav.dashboard")}
      primary={
        <Button size="sm" onClick={() => navigate("/bots/new")}>
          {t("nav.newBot")}
        </Button>
      }
      left={left}
      right={right}
    >
      {!(dismissed && allDone) ? (
        <FirstRunChecklist desk={desk} streamConnected={feed.health.connected} onDismiss={dismiss} />
      ) : null}

      <KpiGrid>
        <KpiTile
          label={t("dashboard.kpi.realizedToday")}
          value={stats === null ? null : stats.totalTrades === 0 ? NO_VALUE : <AnimatedNumber value={stats.todayPnlQuote} format={usdt} />}
          tone={pnlToneAttr(today)}
          note={stats && stats.totalTrades === 0 ? t("dashboard.noTradesToday") : t("dashboard.afterFees", { scope: scopeNote })}
        />
        <KpiTile
          label={t("dashboard.kpi.netAll")}
          value={stats === null ? null : stats.totalTrades === 0 ? NO_VALUE : <AnimatedNumber value={stats.netPnlQuote} format={usdt} />}
          tone={pnlToneAttr(stats?.netPnlQuote)}
          note={stats ? t("dashboard.tradesN", { count: stats.totalTrades }) : undefined}
        />
        <KpiTile
          label={t("dashboard.kpi.dailyLossUsed")}
          value={limit === null || today === null ? null : formatPercent(today < 0 ? (-today / limit) * 100 : 0, locale, 0)}
          note={
            limit !== null && today !== null ? (
              <Meter fraction={today < 0 ? -today / limit : 0} label={t("dashboard.kpi.dailyLossUsed")} />
            ) : undefined
          }
        />
        <KpiTile
          label={t("table.openPositions")}
          value={desk.loaded ? s.openPositions.length : null}
          note={strategyCycles > 0 ? t("dashboard.cyclesNote", { count: strategyCycles }) : undefined}
        />
        <KpiTile
          label={t("dashboard.kpi.capitalInUse")}
          value={desk.loaded ? formatUsdt(capitalInUse, locale, 0) : null}
          note={balance ? t("dashboard.ofBalance", { pct: formatNumber((capitalInUse / balance) * 100, locale, { maximumFractionDigits: 1 }) }) : undefined}
        />
        <KpiTile
          label={t("pnl.winRate")}
          value={stats === null ? null : decided === 0 ? NO_VALUE : formatPercent((stats.wins / decided) * 100, locale, 1)}
          tone={decided > 0 && decided < 30 ? "muted" : undefined}
          note={decided > 0 ? (decided < 30 ? t("states.tooFew", { n: decided }) : `n=${decided}`) : undefined}
        />
      </KpiGrid>

      {desk.loaded && feed.loaded && !feed.health.connected && running.length > 0 ? (
        <p className="ae-banner" data-tone="warn" role="status">
          {t("dashboard.noSignals")}
        </p>
      ) : null}

      <Section
        title={t("dashboard.runningBots")}
        count={desk.loaded ? botRows.length : undefined}
        aside={
          <Link to="/bots" className="ae-link ae-muted">
            {t("nav.allBots")}
          </Link>
        }
      >
        {desk.loaded && botRows.length === 0 ? (
          <EmptyState
            title={t("dashboard.noBots")}
            actions={
              <Button variant="secondary" size="sm" onClick={() => navigate("/bots/new")}>
                {t("nav.newBot")}
              </Button>
            }
          />
        ) : (
          <DataTable
            label={t("dashboard.runningBots")}
            columns={botColumns}
            rows={botRows}
            rowKey={(r) => r.id}
            loading={!desk.loaded}
            compact
            rowNote={(r) => (r.bot && strategyActions.error?.botId === r.bot.id ? strategyActions.error.text : null)}
            rowTone={(r) => (r.bot && strategyActions.error?.botId === r.bot.id ? "danger" : undefined)}
            // The name links to the bot; the row keeps only its run action.
            actions={(r) =>
              r.signal ? (
                <SignalRowAction row={r.signal} />
              ) : r.bot && r.bot.runState !== "dead" ? (
                <Button
                  variant="secondary"
                  size="xs"
                  disabled={strategy.busyId === r.bot.id}
                  onClick={() => r.bot && strategyActions.request(r.bot, isActive(r.bot) ? "stop" : "start")}
                >
                  {t(isActive(r.bot) ? "strategy.actions.stop" : "strategy.actions.start")}
                </Button>
              ) : null
            }
          />
        )}
      </Section>

      <Section
        title={t("dashboard.recentTrades")}
        count={pnl.trades.length > 0 ? Math.min(10, pnl.trades.length) : undefined}
        aside={
          <Link to="/history" className="ae-link ae-muted">
            {t("nav.history")}
          </Link>
        }
      >
        <DataTable
          label={t("dashboard.recentTrades")}
          columns={tradeColumns}
          rows={pnl.trades.slice(0, 10)}
          rowKey={(tr) => String(tr.id)}
          compact
          empty={<span className="ae-subtle">{t("pnl.noTrades")}</span>}
        />
      </Section>
      {strategyActions.dialog}
    </PageShell>
  );
}

/** Read-only slice of the real exchange account (Binance key vaulted). */
function AccountSummary({ exchangeId }: { exchangeId: string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { account, error, updatedAt } = useAccount(exchangeId);
  return (
    <Panel
      title={t("dashboard.exchangeAccount")}
      tone="live"
      aside={
        <Link to="/positions?tab=exchange" className="ae-link ae-muted">
          {t("positions.tabExchange")}
        </Link>
      }
    >
      {account ? (
        <FactList
          rows={[
            { label: t("account.walletBalance"), value: formatUsdt(account.totalWalletBalance, locale) },
            { label: t("account.positions"), value: account.positions.length },
            { label: t("states.asOf"), value: updatedAt ? formatTableTime(updatedAt, locale) : NO_VALUE },
          ]}
        />
      ) : error ? (
        <p className="ae-error">{error}</p>
      ) : (
        <p className="ae-subtle">{t("account.loading")}</p>
      )}
      {account && error ? <p className="ae-error">{error}</p> : null}
    </Panel>
  );
}
