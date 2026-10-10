import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNow } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate, useRouter } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { PnlPanel } from "@/components/PnlPanel/PnlPanel";
import type { PnlController } from "@/components/PnlPanel/usePnl";
import { AnimatedNumber } from "@/components/ui/AnimatedNumber/AnimatedNumber";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FilterGroup, listParam, SearchField } from "@/components/ui/FilterPanel/FilterPanel";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile } from "@/components/ui/KpiTile/KpiTile";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, formatTableTime, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import type { ClosedCycleRow, StrategyPnl } from "@/lib/ipc/strategy/strategy";
import { skipsExportCsv } from "@/lib/ipc/trades/trades";
import { sideText } from "@/lib/strategyText";

const RANGES: Record<string, number> = { "7d": 7, "30d": 30 };

function utcDayStart(now: number): number {
  const d = new Date(now);
  return Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
}

function countBy<T>(items: T[], key: (x: T) => string): [string, number][] {
  const m = new Map<string, number>();
  for (const x of items) m.set(key(x), (m.get(key(x)) ?? 0) + 1);
  return [...m.entries()].sort((a, b) => b[1] - a[1]);
}

/**
 * #/history: closed trades and stats. The stats come from Rust per scope
 * (real and simulated are never summed); the filters narrow the trade list.
 */
export function HistoryPage() {
  const { t } = useTranslation();
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

  const all = pnl.trades;
  const since =
    f.range === "today" ? utcDayStart(now) : RANGES[f.range] ? utcDayStart(now) - (RANGES[f.range] - 1) * 86_400_000 : 0;
  const trades = all.filter(
    (tr) =>
      (!f.q || tr.symbol.toLowerCase().includes(f.q.trim().toLowerCase())) &&
      (!f.kind.length || f.kind.includes(tr.botKind)) &&
      (!f.side.length || f.side.includes(tr.direction)) &&
      (!f.exit.length || f.exit.includes(tr.exitReason)) &&
      tr.closedAt >= since,
  );
  const filtered: PnlController = { ...pnl, trades };
  // Closed DCA / Grid cycles are paper money: shown with the paper scope only.
  const cycleMoney = !pnl.split || pnl.scope === "simulated" ? strategy.pnl : null;
  const cycles = (cycleMoney?.recent ?? []).filter(
    (c) => (!f.q || c.symbol.toLowerCase().includes(f.q.trim().toLowerCase())) && c.closedAt >= since,
  );
  const kinds = countBy(all, (x) => x.botKind);
  const exits = countBy(all, (x) => x.exitReason);

  const left = (
    <>
      <SearchField label={t("filters.symbol")} value={f.q} onChange={(v) => set({ q: v })} />
      <FilterGroup
        label={t("filters.range")}
        options={[
          { value: "today", label: t("filters.todayUtc") },
          { value: "7d", label: "7d" },
          { value: "30d", label: "30d" },
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

  const right = (
    <>
      <Panel title={t("history.byBot")} aside={<span className="ae-subtle tabular">{all.length}</span>}>
        <ul className="ae-list">
          {kinds.map(([k, n]) => (
            <li key={k}>
              <span>{t(`bots.kind.${k}`, { defaultValue: k })}</span>
              <span className="tabular">{n}</span>
            </li>
          ))}
        </ul>
      </Panel>
      <Panel title={t("history.byExit")}>
        <ul className="ae-list">
          {exits.map(([k, n]) => (
            <li key={k}>
              <span>{t(`pnl.exit.${k}`, { defaultValue: k })}</span>
              <span className="tabular">{n}</span>
            </li>
          ))}
        </ul>
      </Panel>
      <p className="ae-subtle">{t("history.loadedScope", { count: all.length })}</p>
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
      <PnlPanel pnl={filtered} />
      {cycleMoney ? <CyclesSection money={cycleMoney} rows={cycles} /> : null}
    </PageShell>
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
