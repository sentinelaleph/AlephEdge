import { useTranslation } from "react-i18next";
import { useNow } from "@/app/alerts";
import { Link } from "@/app/router/router";
import { positionPnlColumns } from "@/components/PositionPnl/PositionPnl";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatAge } from "@/lib/format";
import type { OpenPosition, PositionMark } from "@/lib/ipc/bot/bot";
import { markKey } from "@/lib/positionMarks";

/** How many open positions the Dashboard lists; Positions & orders has all. */
export const PEEK_ROWS = 8;

/** Worst P&L first (what needs a look), positions without a price last. */
export function peekOrder(positions: OpenPosition[], marks: Map<string, PositionMark>): OpenPosition[] {
  const pnl = (p: OpenPosition) => marks.get(markKey(p.botKind, p.signalId))?.pnlUsdt ?? Number.POSITIVE_INFINITY;
  return [...positions].sort((a, b) => pnl(a) - pnl(b));
}

/** Dashboard: the open signal positions at their last price, worst first. */
export function OpenPositionsPeek({ positions, marks }: { positions: OpenPosition[]; marks: Map<string, PositionMark> }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const now = useNow(5_000);
  if (positions.length === 0) return null;
  const rows = peekOrder(positions, marks).slice(0, PEEK_ROWS);
  const pnlCols = positionPnlColumns(t, locale, marks, now).filter((c) => c.id !== "price");
  const columns: DataColumn<OpenPosition>[] = [
    { id: "bot", header: t("table.bot"), priority: 2, cell: (p) => t(`bots.kind.${p.botKind}`) },
    {
      id: "symbol",
      header: t("table.symbol"),
      cell: (p) => (
        <span className="ae-namecell">
          <span aria-hidden="true" className="ae-subtle">
            {p.direction === "short" ? "▼" : "▲"}
          </span>
          {p.symbol}
        </span>
      ),
    },
    ...pnlCols,
    { id: "age", header: t("table.age"), numeric: true, priority: 3, cell: (p) => formatAge((now - p.openedAt) / 1000, locale) },
  ];
  return (
    <Section
      title={t("dashboard.openPositions")}
      count={positions.length}
      aside={
        <Link to="/positions" className="ae-link ae-muted">
          {positions.length > PEEK_ROWS ? t("dashboard.morePositions", { count: positions.length - PEEK_ROWS }) : t("nav.positions")}
        </Link>
      }
    >
      <DataTable label={t("dashboard.openPositions")} columns={columns} rows={rows} rowKey={(p) => markKey(p.botKind, p.signalId)} compact />
    </Section>
  );
}
