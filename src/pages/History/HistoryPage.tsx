import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNow } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate, useRouter } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { PnlPanel } from "@/components/PnlPanel/PnlPanel";
import { AnimatedNumber } from "@/components/ui/AnimatedNumber/AnimatedNumber";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FilterGroup, listParam, SearchField } from "@/components/ui/FilterPanel/FilterPanel";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile } from "@/components/ui/KpiTile/KpiTile";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPercent, formatPnl, formatSignedPercent, formatTableTime, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import type { ClosedCycleRow, StrategyPnl } from "@/lib/ipc/strategy/strategy";
import { isLiveTrade, skipsExportCsv, tradesList, type TradeRecord } from "@/lib/ipc/trades/trades";
import { sideText } from "@/lib/strategyText";
import { aggregateTrades, breakdown, cumulative, dailyNet, DAY_MS, streaks, utcDayStart, type Bucket } from "@/lib/tradeStats";
import { PnlChart } from "./PnlChart";
import "./History.css";

const RANGES: Record<string, number> = { "7d": 7, "30d": 30, "90d": 90 };
/** Trades the page reads; the store keeps every closed trade, this is a safety ceiling. */
export const HISTORY_LIMIT = 10_000;
const POLL_MS = 10_000;
/** Symbols listed in the breakdown (largest results first). */
const SYMBOL_ROWS = 8;

function countBy<T>(items: T[], key: (x: T) => string): [string, number][] {
  const m = new Map<string, number>();
  for (const x of items) m.set(key(x), (m.get(key(x)) ?? 0) + 1);
  return [...m.entries()].sort((a, b) => b[1] - a[1]);
}

/**
 * Every closed trade (not the Dashboard's latest 50), polled. In a live build
 * only the selected scope's money: real and simulated are never mixed.
 */
function useAllTrades(split: boolean, scope: "simulated" | "real"): TradeRecord[] | null {
  const [rows, setRows] = useState<TradeRecord[] | null>(null);
  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const tick = async () => {
      try {
        const all = await tradesList(HISTORY_LIMIT);
        if (alive) setRows(split ? all.filter((tr) => isLiveTrade(tr) === (scope === "real")) : all);
      } catch {
        /* keep last-good */
      }
      if (alive) timer = window.setTimeout(tick, POLL_MS);
    };
    void tick();
    return () => {
      alive = false;
      if (timer) window.clearTimeout(timer);
    };
  }, [split, scope]);
  return rows;
}

/**
 * #/history: closed trades and what they add up to. Every figure on the page
 * (totals, chart, breakdowns, the list) is computed over the trades the
 * filters leave, by the same rules as Rust's stats.
 */
export function HistoryPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const [skipsNote, setSkipsNote] = useState<{ ok: boolean; text: string } | null>(null);
  const exportSkips = async () => {
    setSkipsNote(null);
    try {
      setSkipsNote({ ok: true, text: t("pnl.exportedTo", { path: await skipsExportCsv() }) });
    } catch (e) {
      setSkipsNote({ ok: false, text: errorMessage(e, t("pnl.exportFailed")) });
    }
  };
  const { pnl, strategy } = useDeskContext();
  const { route } = useRouter();
  const now = useNow();
  const q = route.query;
  const f = {
    q: q.get("q") ?? "",
    kind: listParam(q.get("kind")),
    side: listParam(q.get("side")),
    exit: listParam(q.get("exit")),
    range: q.get("range") ?? "",
  };
  const set = (patch: Partial<Record<keyof typeof f, string | string[]>>) => {
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries({ ...f, ...patch })) out[k] = Array.isArray(v) ? v.join(",") : v;
    navigate(withQuery("/history", out), { replace: true });
  };
  const anyFilter = !!(f.q || f.kind.length || f.side.length || f.exit.length || f.range);

  const loaded = useAllTrades(pnl.split, pnl.scope);
  const today = utcDayStart(now);
  const since = f.range === "today" ? today : RANGES[f.range] ? today - (RANGES[f.range] - 1) * DAY_MS : 0;
  const needle = f.q.trim().toLowerCase();
  const kindKey = f.kind.join();
  const sideKey = f.side.join();
  const exitKey = f.exit.join();
  const trades = useMemo(
    () =>
      (loaded ?? []).filter(
        (tr) =>
          (!needle || tr.symbol.toLowerCase().includes(needle)) &&
          (!kindKey || kindKey.split(",").includes(tr.botKind)) &&
          (!sideKey || sideKey.split(",").includes(tr.direction)) &&
          (!exitKey || exitKey.split(",").includes(tr.exitReason)) &&
          tr.closedAt >= since,
      ),
    [loaded, needle, kindKey, sideKey, exitKey, since],
  );
  const all = loaded ?? [];
  const stats = loaded ? aggregateTrades(trades, today) : null;
  const points = useMemo(() => cumulative(trades), [trades]);
  const days = useMemo(() => dailyNet(trades), [trades]);
  const byBot = useMemo(() => breakdown(trades, (x) => x.botKind), [trades]);
  const byExit = useMemo(() => breakdown(trades, (x) => x.exitReason), [trades]);
  const bySymbol = useMemo(() => breakdown(trades, (x) => x.symbol).slice(0, SYMBOL_ROWS), [trades]);
  const run = useMemo(() => streaks(trades), [trades]);

  // Closed DCA / Grid cycles are paper money: shown with the paper scope only.
  const cycleMoney = !pnl.split || pnl.scope === "simulated" ? strategy.pnl : null;
  const cycles = (cycleMoney?.recent ?? []).filter((c) => (!needle || c.symbol.toLowerCase().includes(needle)) && c.closedAt >= since);
  const kinds = countBy(all, (x) => x.botKind);
  const exits = countBy(all, (x) => x.exitReason);
  const longs = trades.filter((x) => x.direction !== "short").length;
  const gains = trades.filter((x) => x.pnlQuote > 0).length;
  const lossesN = trades.filter((x) => x.pnlQuote < 0).length;
  const first = trades.length ? Math.min(...trades.map((x) => x.closedAt)) : null;
  const last = trades.length ? Math.max(...trades.map((x) => x.closedAt)) : null;

  const left = (
    <>
      <SearchField label={t("filters.symbol")} value={f.q} onChange={(v) => set({ q: v })} />
      <FilterGroup
        label={t("filters.range")}
        options={[
          { value: "today", label: t("filters.todayUtc") },
          { value: "7d", label: "7d" },
          { value: "30d", label: "30d" },
          { value: "90d", label: "90d" },
        ]}
        selected={f.range ? [f.range] : []}
        onChange={(v) => set({ range: v[0] ?? "" })}
        single
      />
      <FilterGroup
        label={t("filters.bot")}
        options={kinds.map(([k, n]) => ({ value: k, label: t(`bots.kind.${k}`, { defaultValue: k }), count: n }))}
        selected={f.kind}
        onChange={(v) => set({ kind: v })}
      />
      <FilterGroup
        label={t("filters.direction")}
        options={(["long", "short"] as const).map((d) => ({ value: d, label: t(`signalDesk.direction.${d}`) }))}
        selected={f.side}
        onChange={(v) => set({ side: v })}
      />
      {exits.length > 0 ? (
        <FilterGroup
          label={t("filters.exitReason")}
          options={exits.map(([k, n]) => ({ value: k, label: t(`pnl.exit.${k}`, { defaultValue: k }), count: n }))}
          selected={f.exit}
          onChange={(v) => set({ exit: v })}
        />
      ) : null}
      {anyFilter ? (
        <Button variant="ghost" size="sm" onClick={() => navigate("/history", { replace: true })}>
          {t("states.clearFilters")}
        </Button>
      ) : null}
    </>
  );

  const dayText = (ms: number) => new Date(ms).toLocaleDateString(locale, { day: "numeric", month: "short", year: "numeric" });
  const streakText =
    run.current === null
      ? NO_VALUE
      : t(run.current.kind === "gain" ? "history.streak.gains" : "history.streak.losses", { count: run.current.count });
  const right = (
    <>
      <Panel title={t("history.selection")}>
        <FactList
          rows={[
            { label: t("history.shown"), value: loaded ? t("history.shownOf", { shown: trades.length, total: all.length }) : NO_VALUE },
            {
              label: t("history.period"),
              value: first !== null && last !== null ? `${dayText(first)} – ${dayText(last)}` : NO_VALUE,
            },
            { label: t("history.longShort"), value: trades.length ? `${longs} / ${trades.length - longs}` : NO_VALUE },
            { label: t("history.gainsLosses"), value: trades.length ? `${gains} / ${lossesN}` : NO_VALUE },
          ]}
        />
      </Panel>
      <Panel title={t("history.streaks")}>
        <FactList
          rows={[
            {
              label: t("history.streak.current"),
              value: streakText,
              tone: run.current?.kind === "loss" ? "down" : run.current ? "up" : undefined,
            },
            { label: t("history.streak.longestGain"), value: run.longestGain || NO_VALUE },
            { label: t("history.streak.longestLoss"), value: run.longestLoss || NO_VALUE },
          ]}
        />
        <p className="ae-subtle">{t("history.streak.note")}</p>
      </Panel>
      {loaded && loaded.length >= HISTORY_LIMIT ? <p className="ae-subtle">{t("history.limitNote", { count: HISTORY_LIMIT })}</p> : null}
    </>
  );

  return (
    <PageShell
      title={t("nav.history")}
      crumbs={[{ label: t("nav.groups.portfolio") }]}
      secondary={
        <>
          <Button variant="secondary" size="sm" onClick={() => void pnl.exportCsv()}>
            {t("pnl.exportCsv")}
          </Button>
          <Button variant="secondary" size="sm" onClick={() => void exportSkips()}>
            {t("history.exportSkips")}
          </Button>
        </>
      }
      left={left}
      right={right}
    >
      {skipsNote ? <p className={skipsNote.ok ? "ae-muted" : "ae-error"}>{skipsNote.text}</p> : null}
      {anyFilter ? <p className="ae-muted">{t("history.filtersNote", { shown: trades.length, total: all.length })}</p> : null}
      <PnlPanel pnl={pnl} trades={trades} stats={stats}>
        {trades.length > 0 ? (
          <>
            <Section title={t("history.chart.title")}>
              <PnlChart points={points} days={days} />
            </Section>
            <div className="ae-histgrid">
              <BreakdownTable title={t("history.byBot")} rows={byBot} name={(k) => t(`bots.kind.${k}`, { defaultValue: k })} />
              <BreakdownTable title={t("history.byExit")} rows={byExit} name={(k) => t(`pnl.exit.${k}`, { defaultValue: k })} />
            </div>
            <BreakdownTable title={t("history.bySymbol")} rows={bySymbol} name={(k) => k} />
          </>
        ) : null}
      </PnlPanel>
      {cycleMoney ? <CyclesSection money={cycleMoney} rows={cycles} /> : null}
    </PageShell>
  );
}

/** One breakdown: trades, record, net and average result per group. */
function BreakdownTable({ title, rows, name }: { title: string; rows: Bucket[]; name: (key: string) => string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const columns: DataColumn<Bucket>[] = [
    { id: "name", header: t("table.name"), cell: (b) => name(b.key) },
    { id: "trades", header: t("pnl.trades"), numeric: true, cell: (b) => formatNumber(b.trades, locale) },
    {
      id: "record",
      header: t("history.col.record"),
      headerTip: t("history.col.recordTip"),
      numeric: true,
      priority: 2,
      cell: (b) => (b.wins + b.losses > 0 ? `${b.wins}/${b.losses} · ${formatPercent((b.wins / (b.wins + b.losses)) * 100, locale, 0)}` : NO_VALUE),
    },
    {
      id: "net",
      header: t("pnl.net"),
      numeric: true,
      keep: true,
      cell: (b) => formatPnl(b.net, locale, { unit: "none" }).text,
      tone: (b) => pnlToneAttr(b.net),
    },
    {
      id: "avg",
      header: t("history.col.avg"),
      headerTip: t("history.col.avgTip"),
      numeric: true,
      priority: 2,
      cell: (b) => formatSignedPercent(b.avgPct, locale),
      tone: (b) => pnlToneAttr(b.avgPct),
    },
  ];
  return (
    <Section title={title} count={rows.length}>
      <DataTable label={title} columns={columns} rows={rows} rowKey={(b) => b.key} compact />
    </Section>
  );
}

/**
 * Closed DCA / Grid cycles: their own totals and rows, deleted bots included.
 * Signal trades and cycles are different units (a cycle holds many fills),
 * so they sit in two sections rather than one mixed list.
 */
function CyclesSection({ money, rows }: { money: StrategyPnl; rows: ClosedCycleRow[] }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const usdt = (n: number) => formatPnl(n, locale).text;
  const { totals } = money;
  const columns: DataColumn<ClosedCycleRow>[] = [
    { id: "closed", header: t("table.closedAt"), cell: (c) => formatTableTime(c.closedAt, locale) },
    {
      id: "bot",
      header: t("table.bot"),
      cell: (c) =>
        c.archived ? (
          <span className="ae-namecell">
            {c.botName}
            <Chip>{t("history.cycles.deleted")}</Chip>
          </span>
        ) : (
          <Link to={`/bots/${c.botId}`} className="ae-link">
            {c.botName}
          </Link>
        ),
    },
    { id: "type", header: t("table.type"), priority: 2, cell: (c) => t(`botsList.type.${c.kind}`) },
    { id: "symbol", header: t("table.symbol"), cell: (c) => c.symbol },
    { id: "side", header: t("table.side"), priority: 3, cell: (c) => sideText(t, c.side) },
    {
      id: "exit",
      header: t("table.exitReason"),
      priority: 2,
      cell: (c) => (c.exitReason ? t(`strategy.exit.${c.exitReason}`, { defaultValue: c.exitReason }) : NO_VALUE),
    },
    {
      id: "pnlUsdt",
      header: t("table.pnlUsdt"),
      numeric: true,
      cell: (c) => formatPnl(c.pnlQuote, locale, { unit: "none" }).text,
      tone: (c) => pnlToneAttr(c.pnlQuote),
    },
  ];
  return (
    <Section title={t("history.cycles.title")} count={totals.cycles}>
      {totals.cycles > 0 ? (
        <KpiGrid minTile={140}>
          <KpiTile
            label={t("pnl.net")}
            tone={pnlToneAttr(totals.netQuote)}
            value={<AnimatedNumber value={totals.netQuote} format={usdt} />}
            note={t("history.cycles.netNote")}
          />
          <KpiTile label={t("pnl.today")} tone={pnlToneAttr(totals.todayQuote)} value={<AnimatedNumber value={totals.todayQuote} format={usdt} />} />
          <KpiTile label={t("strategy.col.cycles")} value={formatNumber(totals.cycles, locale)} />
        </KpiGrid>
      ) : null}
      <DataTable
        label={t("history.cycles.title")}
        columns={columns}
        rows={rows}
        rowKey={(c) => `${c.botId}:${c.seq}`}
        compact
        empty={<EmptyState title={t("history.cycles.none")} />}
      />
    </Section>
  );
}
