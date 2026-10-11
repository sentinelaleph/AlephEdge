import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Chip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, formatPrice, formatUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import type { OpenPosition } from "@/lib/ipc/bot/bot";
import { markKey, marksByPosition } from "@/lib/positionMarks";

interface Row {
  p: OpenPosition;
  stop: number | null;
  atStop: number | null;
  price: number | null;
}

/** How far the last price is from the stop, % of the price, on the side the stop is. */
export function stopDistancePct(p: Pick<OpenPosition, "direction">, price: number, stop: number): number {
  return ((p.direction === "short" ? stop - price : price - stop) / price) * 100;
}

/**
 * Where the risk sits: every open position with its current stop, how far
 * the price is from it, and what that stop would book. Largest loss first.
 */
export function OpenRiskTable() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk } = useDeskContext();
  const s = desk.status;
  const marks = marksByPosition(s);
  const at = new Map((s.atStop ?? []).map((a) => [markKey(a.botKind, a.signalId), a]));
  const rows: Row[] = s.openPositions
    .map((p) => {
      const k = markKey(p.botKind, p.signalId);
      const a = at.get(k);
      return { p, stop: a?.stop ?? null, atStop: a?.pnlUsdt ?? null, price: marks.get(k)?.price ?? null };
    })
    .sort((a, b) => (a.atStop ?? 0) - (b.atStop ?? 0));

  const columns: DataColumn<Row>[] = [
    { id: "bot", header: t("table.bot"), priority: 2, cell: (r) => t(`bots.kind.${r.p.botKind}`) },
    {
      id: "symbol",
      header: t("table.symbol"),
      cell: (r) => (
        <span className="ae-namecell">
          <span aria-hidden="true" className="ae-subtle">
            {r.p.direction === "short" ? "▼" : "▲"}
          </span>
          {r.p.symbol}
        </span>
      ),
    },
    {
      id: "stop",
      header: t("safety.open.stop"),
      numeric: true,
      cell: (r) =>
        r.stop === null ? (
          NO_VALUE
        ) : (
          <span className="ae-namecell">
            {r.p.breakevenArmed ? <Chip tone="neutral">{t("safety.open.atEntry")}</Chip> : null}
            {formatPrice(r.stop, locale)}
          </span>
        ),
    },
    {
      id: "distance",
      header: t("safety.open.distance"),
      headerTip: t("safety.open.distanceTip"),
      numeric: true,
      priority: 2,
      cell: (r) =>
        r.price === null || r.stop === null ? NO_VALUE : `${formatNumber(stopDistancePct(r.p, r.price, r.stop), locale, { maximumFractionDigits: 2 })}%`,
    },
    {
      id: "atStop",
      header: t("safety.open.atStop"),
      headerTip: t("safety.open.atStopTip"),
      numeric: true,
      keep: true,
      cell: (r) => (r.atStop === null ? NO_VALUE : formatPnl(r.atStop, locale, { unit: "none" }).text),
      tone: (r) => pnlToneAttr(r.atStop),
    },
    { id: "capital", header: t("table.capital"), numeric: true, priority: 3, cell: (r) => formatUsdt(r.p.capital, locale) },
  ];

  return (
    <Section title={t("safety.section.open")} count={rows.length}>
      <DataTable
        label={t("safety.section.open")}
        columns={columns}
        rows={rows}
        rowKey={(r) => markKey(r.p.botKind, r.p.signalId)}
        compact
        loading={!desk.loaded}
        empty={<EmptyState title={t("safety.summary.noPositions")} />}
      />
    </Section>
  );
}
