import { DataTable } from "@/components/ui/DataTable/DataTable";
import { useTranslation } from "react-i18next";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice } from "@/lib/format";
import type { PreviewDto, StrategyConfig } from "@/lib/ipc/strategy/strategy";
import { strategyErrorText } from "@/lib/strategyText";

interface Props {
  preview: PreviewDto | null;
  /** Raw error of the preview call (no price, …). */
  error: string | null;
  cfg: StrategyConfig;
  /** Strategy budget still free under the cap (this bot's own reservation added back), or null. */
  available: number | null;
  /** Backtest page: no budget-cap check, and funding is not part of the history source. */
  backtest?: boolean;
}

const pct = (frac: number, locale: string) => `${formatNumber(frac * 100, locale, { maximumFractionDigits: 3 })}%`;

/**
 * The derived summary Rust serves for the form (strategy_preview): ladder or
 * grid levels, capital, liquidation, worst case, warnings, fees. Rendered as
 * served; nothing here is recomputed in TypeScript except the budget check.
 */
export function StrategyPreviewPanel({ preview, error, cfg, available, backtest }: Props) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const usdt = (v: number) => `${formatNumber(v, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} USDT`;
  const px = (v: number | null | undefined) => (v === null || v === undefined ? "—" : formatPrice(v, locale));

  if (!preview) {
    return (
      <Panel title={t("strategy.preview.title")}>
        {error ? <p className="ae-error">{strategyErrorText(t, error)}</p> : <p className="ae-subtle">{t("workspace.loading")}</p>}
      </Panel>
    );
  }

  const budgetOk = available === null || cfg.budget <= available + 1e-9;
  const head: Fact[] = [
    { label: t("strategy.preview.referencePrice"), value: px(preview.referencePrice) },
    { label: t("strategy.preview.requiredCapital"), value: usdt(preview.requiredCapital) },
    { label: t("strategy.field.budget"), value: usdt(cfg.budget) },
  ];
  if (!backtest) {
    head.push({
      label: t("strategy.preview.budgetCheck"),
      value:
        available === null
          ? "—"
          : budgetOk
            ? t("strategy.preview.budgetFits", { available: usdt(available) })
            : t("strategy.preview.budgetExceeds", { need: usdt(cfg.budget), available: usdt(Math.max(0, available)) }),
      tone: budgetOk ? undefined : "danger",
    });
  }

  const c = preview.costs;
  const fees: Fact[] = [
    { label: t("strategy.preview.makerFee"), value: pct(c.maker, locale) },
    { label: t("strategy.preview.takerFee"), value: pct(c.taker, locale) },
    { label: t("strategy.preview.slippage"), value: pct(c.slippage, locale) },
    {
      label: t("strategy.preview.funding"),
      value: t(!c.funding ? "strategy.preview.fundingNone" : backtest ? "backtest.label.noFunding" : "strategy.preview.fundingCharged"),
      tone: c.funding && backtest ? "warn" : undefined,
    },
  ];

  return (
    <>
      <Panel title={t("strategy.preview.title")}>
        {preview.error ? (
          <p className="ae-error" role="alert">
            {strategyErrorText(t, preview.error.code)}
          </p>
        ) : null}
        {preview.warnings.length > 0 ? (
          <ul className="ae-sform__warnings">
            {preview.warnings.map((w) => (
              <li key={w}>{t(`strategy.warnings.${w}`, { defaultValue: w })}</li>
            ))}
          </ul>
        ) : null}
        <FactList rows={head} />
      </Panel>

      {preview.dca ? (
        <Panel title={t("strategy.preview.ladder")}>
          <FactList
            rows={[
              { label: t("strategy.preview.coverage"), value: `${formatNumber(preview.dca.maxCoveragePct, locale, { maximumFractionDigits: 2 })}%` },
              { label: t("strategy.preview.lastSoPrice"), value: px(preview.dca.lastSoPrice) },
              { label: t("strategy.preview.totalNotional"), value: usdt(preview.dca.totalNotional) },
              { label: t("strategy.preview.liqPrice"), value: preview.dca.liqPrice === null ? t("strategy.preview.noLiq") : px(preview.dca.liqPrice), tone: preview.dca.liqPrice !== null ? "warn" : undefined },
              ...(preview.dca.liqDistancePct !== null
                ? [{ label: t("strategy.preview.liqDistance"), value: `${formatNumber(preview.dca.liqDistancePct, locale, { maximumFractionDigits: 2 })}%` }]
                : []),
              {
                label: t("strategy.preview.worstLoss"),
                value:
                  preview.dca.worstLossQuote === null
                    ? t("strategy.preview.worstOpen")
                    : `${usdt(preview.dca.worstLossQuote)} · ${t(`strategy.preview.basis.${preview.dca.worstLossBasis}`)}`,
                tone: preview.dca.worstLossQuote === null ? "warn" : "down",
              },
            ]}
          />
        </Panel>
      ) : null}

      {preview.grid ? (
        <Panel title={t("strategy.preview.grid")}>
          <FactList
            rows={[
              { label: t("strategy.preview.range"), value: `${px(preview.grid.lower)} – ${px(preview.grid.upper)}` },
              {
                label: t("strategy.preview.profitPerGrid"),
                value: `${formatNumber(preview.grid.profitPerGridMinPct, locale, { maximumFractionDigits: 3 })}% – ${formatNumber(preview.grid.profitPerGridMaxPct, locale, { maximumFractionDigits: 3 })}%`,
                tone: preview.grid.profitPerGridMinPct < 0.3 ? "warn" : undefined,
              },
              { label: t("strategy.preview.ordersNow"), value: t("strategy.preview.buySell", { buy: preview.grid.buyOrders, sell: preview.grid.sellOrders }) },
              { label: t("strategy.preview.qtyPerLevel"), value: formatNumber(preview.grid.qtyPerLevel, locale, { maximumSignificantDigits: 6 }) },
              {
                label: t("strategy.preview.initialBase"),
                value:
                  preview.grid.initialBaseQty > 0
                    ? `${formatNumber(preview.grid.initialBaseQty, locale, { maximumSignificantDigits: 6 })} · ${usdt(preview.grid.initialQuote)}`
                    : "—",
              },
              {
                label: t("strategy.preview.stopBand"),
                value:
                  preview.grid.stopLower === null && preview.grid.stopUpper === null
                    ? t("strategy.preview.noStop")
                    : `${px(preview.grid.stopLower)} / ${px(preview.grid.stopUpper)}`,
                tone: preview.grid.stopLower === null && preview.grid.stopUpper === null ? "warn" : undefined,
              },
              {
                label: t("strategy.preview.liqBand"),
                value:
                  preview.grid.liqPriceBottom === null && preview.grid.liqPriceTop === null
                    ? t("strategy.preview.noLiq")
                    : `${px(preview.grid.liqPriceBottom)} / ${px(preview.grid.liqPriceTop)}`,
                tone: preview.grid.liqPriceBottom !== null || preview.grid.liqPriceTop !== null ? "warn" : undefined,
              },
            ]}
          />
        </Panel>
      ) : null}

      <Panel title={t("strategy.preview.costs")}>
        <FactList rows={fees} />
      </Panel>
    </>
  );
}

/**
 * The ladder (DCA) or level table (Grid) of the Rust preview, for the center
 * column: the summary rail is too narrow for six numeric columns.
 */
export function StrategyLevelsTable({ preview }: { preview: PreviewDto | null }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const px = (v: number | null | undefined) => (v === null || v === undefined ? "—" : formatPrice(v, locale));
  if (!preview) return null;
  if (preview.dca) {
    const withLiq = preview.dca.rungs.some((r) => r.liqPrice !== null);
    return (
      <Panel title={t("strategy.preview.ladder")}>
        <DataTable
          label={t("strategy.preview.ladder")}
          bare
          compact
          rows={preview.dca.rungs}
          rowKey={(r) => String(r.index)}
          columns={[
            { id: "rung", header: "#", keep: true, cell: (r) => (r.index === 0 ? t("strategy.preview.base") : `SO${r.index}`) },
            { id: "dev", header: t("strategy.preview.deviation"), numeric: true, priority: 2, cell: (r) => `${formatNumber(r.deviationPct, locale, { maximumFractionDigits: 2 })}%` },
            { id: "price", header: t("table.price"), numeric: true, cell: (r) => px(r.price) },
            { id: "notional", header: t("table.notional"), numeric: true, cell: (r) => formatNumber(r.notional, locale, { maximumFractionDigits: 2 }) },
            { id: "avg", header: t("strategy.preview.avgEntry"), numeric: true, priority: 3, cell: (r) => px(r.avgEntry) },
            { id: "tp", header: t("strategy.preview.tpPrice"), numeric: true, priority: 2, cell: (r) => px(r.tpPrice) },
            ...(withLiq
              ? [{ id: "liq", header: t("strategy.preview.liqPrice"), numeric: true, priority: 3 as const, cell: (r: (typeof preview.dca.rungs)[number]) => px(r.liqPrice) }]
              : []),
          ]}
        />
      </Panel>
    );
  }
  if (preview.grid) {
    return (
      <Panel title={t("strategy.preview.grid")}>
        <DataTable
          label={t("strategy.preview.grid")}
          bare
          compact
          rows={[...preview.grid.levels].reverse()}
          rowKey={(l) => String(l.index)}
          columns={[
            { id: "level", header: "#", keep: true, cell: (l) => String(l.index) },
            { id: "price", header: t("table.price"), numeric: true, cell: (l) => px(l.price) },
            {
              id: "side",
              header: t("table.side"),
              cell: (l) => t(`strategy.preview.levelSide.${l.side}`),
              tone: (l) => (l.side === "buy" ? "up" : l.side === "sell" ? "down" : "muted"),
            },
            { id: "notional", header: t("table.notional"), numeric: true, cell: (l) => formatNumber(l.notional, locale, { maximumFractionDigits: 2 }) },
          ]}
        />
      </Panel>
    );
  }
  return null;
}
