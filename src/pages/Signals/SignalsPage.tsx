import { useTranslation } from "react-i18next";
import { useNow } from "@/app/alerts";
import { useDeskContext } from "@/app/DeskProvider";
import { navigate, useRouter } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { StreamCard } from "@/components/Desk/Readouts";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FilterGroup, listParam, SearchField } from "@/components/ui/FilterPanel/FilterPanel";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { Tooltip } from "@/components/ui/Tooltip/Tooltip";
import { localeForLanguage } from "@/i18n";
import { formatAge, formatNumber, formatPrice, formatTableTime, NO_VALUE } from "@/lib/format";
import type { Signal } from "@/lib/ipc/signal/signal";
import { SkeletonRows } from "@/pages/Bots/SkeletonRows";
import "./SignalsPage.css";
import { streamAlertSeverity, streamErrorText, streamLabelKey } from "@/lib/ipc/signal/streamStatus";

type Status = "active" | "expired";
const AGE_WINDOWS: Record<string, number> = { "1h": 3_600_000, "4h": 14_400_000, "24h": 86_400_000 };
const MIN_CONF = ["50", "70"];

function statusOf(s: Signal, now: number): Status {
  return new Date(s.expires_at).getTime() > now ? "active" : "expired";
}

/** Sources of the signal's confluence, primary first (the "combo"). */
function comboOf(s: Signal): string {
  return s.combo || s.confluence.map((c) => c.source).join(" + ") || s.mode;
}

/**
 * The combo as it fits a table cell: whole source names only, never cut
 * mid-word. More than two sources show the first two and "+N"; the full list
 * is the cell's tooltip.
 */
export function comboShort(full: string): string {
  const parts = full.split(/\s*\+\s*/).filter(Boolean);
  if (parts.length <= 2) return full;
  return `${parts[0]} + ${parts[1]} +${parts.length - 2}`;
}

const two = (v: number, locale: string) => formatNumber(v, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/**
 * #/signals: the Sentinel feed as a filterable table (the app-level stream,
 * buffered by Rust), with the selected signal's detail and what the desk's
 * bots did with it.
 */
export function SignalsPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { feed, desk, pnl } = useDeskContext();
  const { route } = useRouter();
  const now = useNow(15_000);
  const q = route.query;
  const f = {
    q: q.get("q") ?? "",
    dir: listParam(q.get("dir")),
    tf: listParam(q.get("tf")),
    status: listParam(q.get("status")),
    conf: q.get("conf") ?? "",
    age: q.get("age") ?? "",
    id: q.get("id") ?? "",
  };
  const set = (patch: Partial<Record<keyof typeof f, string | string[]>>) => {
    const merged = { ...f, ...patch };
    const out: Record<string, string> = {};
    for (const [k, v] of Object.entries(merged)) out[k] = Array.isArray(v) ? v.join(",") : v;
    navigate(withQuery("/signals", out), { replace: true });
  };
  const anyFilter = !!(f.q || f.dir.length || f.tf.length || f.status.length || f.conf || f.age);

  const all = feed.signals;
  const timeframes = [...new Set(all.map((s) => s.timeframe).filter((x): x is string => !!x))];
  const rows = all.filter((s) => {
    if (f.q && !s.symbol.toLowerCase().includes(f.q.trim().toLowerCase())) return false;
    if (f.dir.length && !f.dir.includes(s.direction)) return false;
    if (f.tf.length && !f.tf.includes(s.timeframe ?? "")) return false;
    if (f.status.length && !f.status.includes(statusOf(s, now))) return false;
    if (f.conf && s.confidence * 100 < Number(f.conf)) return false;
    if (f.age && now - new Date(s.created_at).getTime() > (AGE_WINDOWS[f.age] ?? Infinity)) return false;
    return true;
  });
  const selected = all.find((s) => s.id === f.id) ?? null;
  const last24 = all.filter((s) => now - new Date(s.created_at).getTime() <= AGE_WINDOWS["24h"]);

  const columns: DataColumn<Signal>[] = [
    {
      id: "time",
      header: t("table.time"),
      cell: (s) => <span className="tabular">{formatTableTime(s.created_at, locale, now)}</span>,
    },
    {
      id: "symbol",
      header: t("table.symbol"),
      keep: true,
      cell: (s) => <span className="ae-sigtable__sym">{s.symbol}</span>,
    },
    {
      id: "tf",
      header: "TF",
      headerTip: t("filters.timeframe"),
      detailLabel: t("filters.timeframe"),
      priority: 2,
      cell: (s) => s.timeframe ?? NO_VALUE,
    },
    {
      id: "side",
      header: t("table.side"),
      cell: (s) => <Chip tone={s.direction === "long" ? "success" : "danger"}>{t(`signalDesk.direction.${s.direction}`)}</Chip>,
    },
    { id: "entry", header: t("table.entry"), numeric: true, cell: (s) => formatPrice(s.entry, locale) },
    ...[0, 1, 2].map(
      (i): DataColumn<Signal> => ({
        id: `tp${i + 1}`,
        header: `TP${i + 1}`,
        numeric: true,
        priority: i === 0 ? 1 : i === 1 ? 2 : 3,
        cell: (s) => (s.tp[i] != null ? formatPrice(s.tp[i], locale) : NO_VALUE),
      }),
    ),
    { id: "sl", header: t("table.sl"), numeric: true, cell: (s) => formatPrice(s.sl, locale) },
    {
      id: "evidence",
      header: t("signalsPage.evidence"),
      headerTip: t("signalDesk.evidenceScoreTooltip"),
      numeric: true,
      priority: 2,
      cell: (s) => Math.round(s.confidence * 100),
    },
    {
      id: "combo",
      header: t("signalsPage.combo"),
      priority: 3,
      cell: (s) => {
        const full = comboOf(s);
        const short = comboShort(full);
        return short === full ? (
          full
        ) : (
          <Tooltip content={full}>
            <span tabIndex={0} className="ae-sigtable__combo">
              {short}
            </span>
          </Tooltip>
        );
      },
    },
    {
      id: "state",
      header: t("table.state"),
      cell: (s) => {
        const st = statusOf(s, now);
        return <StatusChip status={st === "active" ? "running" : "stopped"} label={t(`signalsPage.status.${st}`)} />;
      },
    },
    {
      id: "age",
      header: t("table.age"),
      numeric: true,
      priority: 2,
      cell: (s) => formatAge((now - new Date(s.created_at).getTime()) / 1000, locale),
    },
  ];

  const left = (
    <>
      <SearchField label={t("filters.symbol")} value={f.q} onChange={(v) => set({ q: v })} />
      <FilterGroup
        label={t("filters.direction")}
        options={(["long", "short"] as const).map((d) => ({ value: d, label: t(`signalDesk.direction.${d}`) }))}
        selected={f.dir}
        onChange={(v) => set({ dir: v })}
      />
      {timeframes.length > 0 ? (
        <FilterGroup
          label={t("filters.timeframe")}
          options={timeframes.map((tf) => ({ value: tf, label: tf }))}
          selected={f.tf}
          onChange={(v) => set({ tf: v })}
        />
      ) : null}
      <FilterGroup
        label={t("filters.state")}
        options={(["active", "expired"] as const).map((s) => ({ value: s, label: t(`signalsPage.status.${s}`) }))}
        selected={f.status}
        onChange={(v) => set({ status: v })}
      />
      <FilterGroup
        label={t("filters.minEvidence")}
        options={MIN_CONF.map((c) => ({ value: c, label: `≥ ${c}` }))}
        selected={f.conf ? [f.conf] : []}
        onChange={(v) => set({ conf: v[0] ?? "" })}
        single
      />
      <FilterGroup
        label={t("filters.age")}
        options={Object.keys(AGE_WINDOWS).map((a) => ({ value: a, label: a }))}
        selected={f.age ? [f.age] : []}
        onChange={(v) => set({ age: v[0] ?? "" })}
        single
      />
      {anyFilter ? (
        <Button variant="ghost" size="sm" onClick={() => navigate(withQuery("/signals", { id: f.id }), { replace: true })}>
          {t("states.clearFilters")}
        </Button>
      ) : null}
    </>
  );

  const opened = selected ? desk.status.openPositions.filter((p) => p.signalId === selected.id) : [];
  const closed = selected ? pnl.trades.filter((tr) => tr.signalId === selected.id) : [];

  const right = (
    <>
      <StreamCard />
      <Panel title={t("signalsPage.last24h")}>
        <FactList
          rows={[
            { label: t("signalsPage.status.active"), value: last24.filter((s) => statusOf(s, now) === "active").length },
            { label: t("signalsPage.status.expired"), value: last24.filter((s) => statusOf(s, now) === "expired").length },
            { label: t("signalDesk.direction.long"), value: last24.filter((s) => s.direction === "long").length },
            { label: t("signalDesk.direction.short"), value: last24.filter((s) => s.direction === "short").length },
          ]}
        />
      </Panel>
      {selected ? (
        <>
          <Panel title={`${selected.symbol} ${selected.timeframe ?? ""}`} aside={<Chip tone={selected.direction === "long" ? "success" : "danger"}>{t(`signalDesk.direction.${selected.direction}`)}</Chip>}>
            <FactList
              rows={[
                { label: t("table.entry"), value: formatPrice(selected.entry, locale) },
                ...selected.tp.map((tp, i) => ({ label: `TP${i + 1}`, value: formatPrice(tp, locale) })),
                { label: t("table.sl"), value: formatPrice(selected.sl, locale) },
                { label: "R:R", value: two(selected.rr, locale) },
                {
                  label: t("signalsPage.evidence"),
                  value: `${Math.round(selected.confidence * 100)}/100`,
                  info: t("signalDesk.evidenceScoreTooltip"),
                },
                { label: t("signalsPage.expiresAt"), value: formatTableTime(selected.expires_at, locale, now) },
              ]}
            />
            {selected.confluence.length > 0 ? (
              <>
                <h3 className="ae-section-title">{t("signalsPage.confluence")}</h3>
                <ul className="ae-list">
                  {selected.confluence.map((c) => (
                    <li key={c.source}>
                      <span>{c.source}</span>
                      <span className="tabular">{two(c.weighted, locale)}</span>
                    </li>
                  ))}
                </ul>
              </>
            ) : null}
            {selected.management_plan ? (
              <>
                <h3 className="ae-section-title">{t("signalsPage.plan")}</h3>
                <FactList
                  rows={[
                    { label: t("signalsPage.breakevenAt"), value: selected.management_plan.breakeven_at_r != null ? `${formatNumber(selected.management_plan.breakeven_at_r, locale)}R` : NO_VALUE },
                    { label: t("signalsPage.partialAt"), value: selected.management_plan.partial_at_r != null ? `${formatNumber(selected.management_plan.partial_at_r, locale)}R` : NO_VALUE },
                  ]}
                />
              </>
            ) : null}
          </Panel>
          <Panel title={t("signalsPage.decisions")}>
            {opened.length === 0 && closed.length === 0 ? (
              <p className="ae-subtle">{t("signalsPage.noEntry")}</p>
            ) : (
              <ul className="ae-list">
                {opened.map((p) => (
                  <li key={`o:${p.botKind}`}>
                    <span>{t(`bots.kind.${p.botKind}`)}</span>
                    <span>{t("signalsPage.openedOpen")}</span>
                  </li>
                ))}
                {closed.map((tr) => (
                  <li key={`c:${tr.id}`}>
                    <span>{t(`bots.kind.${tr.botKind}`, { defaultValue: tr.botKind })}</span>
                    <span>{t(`pnl.exit.${tr.exitReason}`, { defaultValue: tr.exitReason })}</span>
                  </li>
                ))}
              </ul>
            )}
            <p className="ae-subtle">{t("signalsPage.decisionsScope")}</p>
          </Panel>
        </>
      ) : (
        <p className="ae-subtle">{t("signalsPage.selectHint")}</p>
      )}
    </>
  );

  return (
    <PageShell title={t("nav.signals")} crumbs={[{ label: t("nav.groups.market") }]} left={left} right={right}>
      {feed.loaded && streamAlertSeverity(feed.health) && streamErrorText(feed.health) ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {streamErrorText(feed.health)}
        </p>
      ) : null}
      {!feed.loaded ? (
        <SkeletonRows rows={8} />
      ) : all.length === 0 ? (
        <EmptyState
          title={t("signalsPage.empty")}
          detail={t(streamLabelKey(feed.health))}
        />
      ) : rows.length === 0 ? (
        <EmptyState
          title={t("signalsPage.noMatch")}
          actions={
            <Button variant="secondary" size="sm" onClick={() => navigate("/signals", { replace: true })}>
              {t("states.clearFilters")}
            </Button>
          }
        />
      ) : (
        <DataTable
          label={t("nav.signals")}
          columns={columns}
          rows={rows}
          rowKey={(s) => s.id}
          isSelected={(s) => s.id === f.id}
          onRowActivate={(s) => set({ id: s.id })}
          compact
        />
      )}
    </PageShell>
  );
}
