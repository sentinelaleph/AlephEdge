import { useTranslation } from "react-i18next";
import { useNow } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { navigate, useRouter } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { PnlPanel } from "@/components/PnlPanel/PnlPanel";
import type { PnlController } from "@/components/PnlPanel/usePnl";
import { Button } from "@/components/ui/Button/Button";
import { FilterGroup, listParam, SearchField } from "@/components/ui/FilterPanel/FilterPanel";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { Panel } from "@/components/ui/Panel/Panel";

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
  const { pnl } = useDeskContext();
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
        <Button variant="secondary" size="sm" onClick={() => void pnl.exportCsv()}>
          {t("pnl.exportCsv")}
        </Button>
      }
      left={left}
      right={right}
    >
      {anyFilter ? <p className="ae-muted">{t("history.filtersNote", { shown: trades.length, total: all.length })}</p> : null}
      <PnlPanel pnl={filtered} />
    </PageShell>
  );
}
