import { useTranslation } from "react-i18next";
import { AnimatedNumber } from "@/components/ui/AnimatedNumber/AnimatedNumber";
import { LiveChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile } from "@/components/ui/KpiTile/KpiTile";
import { Section } from "@/components/ui/Section/Section";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { Tooltip } from "@/components/ui/Tooltip/Tooltip";
import { isLiveTrade, type TradeRecord } from "@/lib/ipc/trades/trades";
import { localeForLanguage } from "@/i18n";
import {
  formatNumber,
  formatPnl,
  formatPrice,
  formatSignedPercent,
  formatSignedUsdt,
  formatTableTime,
  formatUsdt,
  NO_VALUE,
  pnlToneAttr,
} from "@/lib/format";
import { FUNDING_COST_FRAC, ROUND_TRIP_FEE_FRAC } from "@/lib/sizing";
import { targetLabel } from "@/lib/takeProfit";
import type { PnlController, PnlScope } from "./usePnl";
import "./PnlPanel.css";

const SCOPES: PnlScope[] = ["simulated", "real"];

/**
 * Exit reasons that carry a short explanatory tooltip (the newer,
 * less self-evident ones). "tp"/"sl"/"expired" etc. don't need one; historical
 * reasons the engine no longer produces ("invalidated", "btcBreakCap") keep
 * their old label but no tooltip. "veto" is handled separately below — its
 * tooltip is Sentinel's own per-trade reason text, not a static string.
 */
const EXIT_REASONS_WITH_TOOLTIP = new Set([
  "breakeven",
  "horizon",
  "max_loss",
  "exchangeClosed",
  "veto",
  "unprotected",
  "liquidation",
  "stopped",
]);

interface PnlPanelProps {
  pnl: PnlController;
}

/**
 * Stats and closed trades (PRD §5.6): net PnL (fees in), qualified winrate,
 * profit factor, max drawdown, and the trade list with the FR/LD context
 * captured at open. The page header carries Export CSV; the page title names
 * the section, so there is no heading here.
 */
export function PnlPanel({ pnl }: PnlPanelProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const s = pnl.stats;
  const costParams = {
    fees: formatNumber(ROUND_TRIP_FEE_FRAC * 100, locale, { minimumFractionDigits: 2 }),
    funding: formatNumber(FUNDING_COST_FRAC * 100, locale, { minimumFractionDigits: 2 }),
  };
  const usdt = (n: number) => formatPnl(n, locale).text;

  const exitTip = (tr: TradeRecord): string | undefined =>
    tr.exitReason === "veto" && (tr.vetoReasonCode || tr.vetoReasonText)
      ? t(`bots.vetoReason.${tr.vetoReasonCode ?? ""}`, { defaultValue: tr.vetoReasonText ?? tr.vetoReasonCode ?? "" })
      : EXIT_REASONS_WITH_TOOLTIP.has(tr.exitReason)
        ? t(`pnl.exitTooltip.${tr.exitReason}`)
        : undefined;

  const details = (tr: TradeRecord): string => {
    const parts: string[] = [];
    if (tr.tpTarget) parts.push(t("bots.position.target", { target: targetLabel(tr.tpTarget, t("bots.tp.custom")) }));
    if (tr.tpFallbackFrom)
      parts.push(
        t("bots.position.fallback", {
          from: targetLabel(tr.tpFallbackFrom, t("bots.tp.custom")),
          to: targetLabel(tr.tpTarget, t("bots.tp.custom")),
        }),
      );
    if (tr.frAtOpen != null) parts.push(`FR ${formatNumber(tr.frAtOpen, locale, { minimumFractionDigits: 3, maximumFractionDigits: 3 })}%`);
    if (tr.ldAtOpen != null) parts.push(`LD ${formatUsdt(tr.ldAtOpen, locale, 0)}`);
    if (tr.live && tr.fillEntry != null && tr.fillExit != null)
      parts.push(t("pnl.fill", { entry: formatPrice(tr.fillEntry, locale), exit: formatPrice(tr.fillExit, locale) }));
    if (tr.live && tr.commissionUsdt != null)
      parts.push(t("pnl.commission", { amount: formatNumber(tr.commissionUsdt, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) }));
    return parts.length ? parts.join(" · ") : NO_VALUE;
  };

  const columns: DataColumn<TradeRecord>[] = [
    {
      id: "closed",
      header: t("table.closedAt"),
      cell: (tr) => <span className="tabular">{formatTableTime(tr.closedAt, locale)}</span>,
    },
    {
      id: "symbol",
      header: t("table.symbol"),
      cell: (tr) => (
        <span className="ae-pnl__sym">
          {tr.symbol}
          {isLiveTrade(tr) ? <LiveChip /> : null}
        </span>
      ),
    },
    {
      id: "side",
      header: t("table.side"),
      cell: (tr) => t(`signalDesk.direction.${tr.direction === "short" ? "short" : "long"}`),
    },
    { id: "bot", header: t("table.bot"), priority: 2, cell: (tr) => t(`bots.kind.${tr.botKind}`, { defaultValue: tr.botKind }) },
    {
      id: "exit",
      header: t("table.exitReason"),
      priority: 2,
      cell: (tr) => {
        const label = t(`pnl.exit.${tr.exitReason}`, { defaultValue: tr.exitReason });
        const tip = exitTip(tr);
        return tip ? (
          <Tooltip content={tip}>
            <span tabIndex={0} className="ae-pnl__tip">
              {label}
            </span>
          </Tooltip>
        ) : (
          label
        );
      },
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
      headerTip: t("pnl.accountScaleTooltip"),
      numeric: true,
      cell: (tr) => formatPnl(tr.pnlUsdt, locale, { unit: "none" }).text,
      tone: (tr) => pnlToneAttr(tr.pnlUsdt),
    },
    { id: "entry", header: t("table.entry"), numeric: true, detailOnly: true, cell: (tr) => formatPrice(tr.entry, locale) },
    { id: "exitPx", header: t("table.exit"), numeric: true, detailOnly: true, cell: (tr) => formatPrice(tr.exit, locale) },
    { id: "leverage", header: t("table.leverage"), numeric: true, detailOnly: true, cell: (tr) => `${tr.leverage}x` },
    {
      id: "ledger",
      header: t("pnl.col.ledger"),
      numeric: true,
      detailOnly: true,
      cell: (tr) =>
        tr.unleveredNetPct != null ? (
          <Tooltip content={t("pnl.ledgerComparableTooltip", costParams)}>
            <span tabIndex={0} className="ae-pnl__tip">
              {formatSignedPercent(tr.unleveredNetPct, locale)}
            </span>
          </Tooltip>
        ) : (
          NO_VALUE
        ),
      tone: (tr) => pnlToneAttr(tr.unleveredNetPct),
    },
    { id: "details", header: t("table.details"), detailOnly: true, wrap: true, cell: details },
  ];

  const dd = s && s.maxDrawdownQuote >= 0.005 ? s.maxDrawdownQuote : 0;

  return (
    <section className="ae-pnl">
      {pnl.exportedTo ? <p className="ae-pnl__exported">{t("pnl.exportedTo", { path: pnl.exportedTo })}</p> : null}
      {pnl.exportError ? (
        <p className="ae-error" role="alert">
          {pnl.exportError}
        </p>
      ) : null}

      {/* A live build shows real and simulated money apart, one scope at a
          time; the two are never added together. */}
      {pnl.split ? (
        <div className="ae-pnl__scope">
          <SegmentedControl
            label={t("pnl.scope.label")}
            size="sm"
            wrap
            value={pnl.scope}
            onChange={pnl.setScope}
            options={SCOPES.map((sc) => ({
              value: sc,
              label: t(`pnl.scope.${sc}`),
              indicator: sc === "real" ? "var(--mode-live)" : undefined,
            }))}
          />
        </div>
      ) : null}

      {s ? (
        <KpiGrid minTile={140}>
          <KpiTile label={t("pnl.net")} tone={pnlToneAttr(s.netPnlQuote)} value={<AnimatedNumber value={s.netPnlQuote} format={usdt} />} />
          <KpiTile label={t("pnl.winRate")} value={<WinRate wins={s.wins} losses={s.losses} locale={locale} />} />
          <KpiTile
            label={t("pnl.profitFactor")}
            value={s.profitFactor === null ? NO_VALUE : formatNumber(s.profitFactor, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}
          />
          {/* A drawdown under half a cent is none: "-0.00" in red reads as a loss. */}
          <KpiTile label={t("pnl.maxDrawdown")} value={dd > 0 ? formatSignedUsdt(-dd, locale) : formatUsdt(0, locale)} tone={dd > 0 ? "down" : undefined} />
          <KpiTile label={t("pnl.trades")} value={formatNumber(s.totalTrades, locale)} />
          <KpiTile label={t("pnl.today")} tone={pnlToneAttr(s.todayPnlQuote)} value={<AnimatedNumber value={s.todayPnlQuote} format={usdt} />} />
        </KpiGrid>
      ) : null}

      <Section title={t("pnl.trades")} count={pnl.trades.length}>
      <DataTable
        label={t("pnl.trades")}
        columns={columns}
        rows={pnl.trades}
        rowKey={(tr) => String(tr.id)}
        empty={<EmptyState title={t("pnl.noTrades")} />}
      />
      </Section>
    </section>
  );
}

/**
 * The win rate is meaningless without its own denominator: "62.5%" reads as
 * confident, "62.5% · n=40" reads as what it is — 40 closed trades that
 * actually resolved to a win or a loss (not the total trade count, which
 * also includes non-TP/SL exits). Zero settled trades shows "—", never 0%.
 */
function WinRate({ wins, losses, locale }: { wins: number; losses: number; locale: string }) {
  const { t } = useTranslation();
  const n = wins + losses;
  if (n === 0) return <>{NO_VALUE}</>;
  const pct = formatNumber((wins / n) * 100, locale, { minimumFractionDigits: 1, maximumFractionDigits: 1 });
  return <>{t("pnl.winRateValue", { pct, n })}</>;
}
