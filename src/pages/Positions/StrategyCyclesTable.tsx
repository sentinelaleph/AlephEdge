import { useTranslation } from "react-i18next";
import { Link } from "@/app/router/router";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { localeForLanguage } from "@/i18n";
import { formatPrice, formatSignedUsdt, formatTableTime, formatUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import type { StrategyBotView } from "@/lib/ipc/strategy/strategy";
import { sideText } from "@/lib/strategyText";

/**
 * Open DCA / Grid cycles (paper only; the header's mode chip says so), one
 * row per bot holding a cycle. Every figure is the bot's own view from Rust
 * (strategy_list); nothing is derived.
 */
export function StrategyCyclesTable({ bots }: { bots: StrategyBotView[] }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const rows = bots.filter((b) => b.openCycle !== null);
  const columns: DataColumn<StrategyBotView>[] = [
    {
      id: "bot",
      header: t("table.bot"),
      cell: (b) => (
        <Link to={`/bots/${encodeURIComponent(b.id)}`} className="ae-link">
          {b.name}
        </Link>
      ),
    },
    { id: "type", header: t("table.type"), priority: 2, cell: (b) => t(`botsList.type.${b.kind}`) },
    { id: "symbol", header: t("table.symbol"), cell: (b) => b.symbol },
    { id: "side", header: t("table.side"), priority: 2, cell: (b) => sideText(t, b.side) },
    {
      id: "avgEntry",
      header: t("strategy.preview.avgEntry"),
      numeric: true,
      cell: (b) => (b.openCycle!.avgEntry !== null ? formatPrice(b.openCycle!.avgEntry, locale) : NO_VALUE),
    },
    { id: "notional", header: t("table.notional"), numeric: true, priority: 3, cell: (b) => formatUsdt(b.openCycle!.notional, locale) },
    {
      id: "unrealized",
      header: t("strategy.detail.unrealized"),
      numeric: true,
      cell: (b) => formatSignedUsdt(b.openCycle!.unrealizedQuote, locale),
      tone: (b) => pnlToneAttr(b.openCycle!.unrealizedQuote),
    },
    {
      id: "opened",
      header: t("strategy.detail.opened"),
      priority: 2,
      cell: (b) => <span className="tabular">{formatTableTime(b.openCycle!.openedAt, locale)}</span>,
    },
  ];
  return (
    <DataTable
      label={t("positions.strategyCycles")}
      columns={columns}
      rows={rows}
      rowKey={(b) => b.id}
      empty={<EmptyState title={t("positions.noStrategyCycles")} />}
    />
  );
}
