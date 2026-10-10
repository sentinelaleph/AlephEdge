import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice } from "@/lib/format";
import type { PreviewDto, StrategyConfig } from "@/lib/ipc/strategy/strategy";

/**
 * A reachable liquidation with no stop in front of it: past the last safety
 * order (DCA, stop loss off) or outside the range (grid, stop-out off)
 * nothing closes the position before the exchange liquidates it, and the
 * cycle's whole margin is gone. Owner's backtest 3 Oct: 5x, no stop, one
 * cycle at -100% of budget; the two 1x short DCA templates were liquidated
 * 6 times in their test. Such a config is allowed, but only after the user
 * has read the liquidation price and confirmed it.
 *
 * The Rust preview says whether a liquidation price exists (a 1x short has
 * one, a 1x long has none). Without a preview (no price yet), leverage above
 * 1x, a short DCA or a short / neutral grid stands in for it.
 */
export function needsLiquidationConfirm(cfg: StrategyConfig, preview: PreviewDto | null): boolean {
  const p = cfg.params;
  if (p.kind === "dca") {
    if (p.slPct !== null) return false;
    return preview?.dca ? preview.dca.liqPrice !== null : cfg.leverage > 1 || cfg.side === "short";
  }
  if (p.stopOutPct !== null) return false;
  return preview?.grid
    ? preview.grid.liqPriceBottom !== null || preview.grid.liqPriceTop !== null
    : cfg.leverage > 1 || cfg.side !== "long";
}

interface LeverageRiskDialogProps {
  open: boolean;
  cfg: StrategyConfig;
  preview: PreviewDto | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export function LeverageRiskDialog({ open, cfg, preview, onConfirm, onCancel }: LeverageRiskDialogProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const px = (v: number | null | undefined) => (v === null || v === undefined ? "—" : formatPrice(v, locale));
  let body: string;
  let hint: string;
  if (cfg.params.kind === "grid") {
    const g = preview?.grid ?? null;
    body =
      g && (g.liqPriceBottom !== null || g.liqPriceTop !== null)
        ? t("strategy.leverageConfirm.body_grid", { leverage: cfg.leverage, price: `${px(g.liqPriceBottom)} / ${px(g.liqPriceTop)}` })
        : t("strategy.leverageConfirm.bodyGenericGrid", { leverage: cfg.leverage });
    hint = t("strategy.leverageConfirm.hintGrid");
  } else {
    const dca = preview?.dca ?? null;
    const pct = dca?.liqDistancePct;
    const liq = dca?.liqPrice;
    const side = cfg.side === "short" ? "short" : "long";
    body =
      pct != null && liq != null
        ? t(`strategy.leverageConfirm.body_${side}`, {
            leverage: cfg.leverage,
            pct: formatNumber(Math.abs(pct), locale, { maximumFractionDigits: 1 }),
            price: formatPrice(liq, locale),
          })
        : t("strategy.leverageConfirm.bodyGeneric", { leverage: cfg.leverage });
    // At 1x a long cannot be liquidated; a short can (near twice its average).
    hint = side === "short" ? t("strategy.leverageConfirm.hintShort") : t("strategy.leverageConfirm.hint");
  }
  return (
    <ConfirmDialog
      open={open}
      title={t("strategy.leverageConfirm.title")}
      body={
        <>
          <p>{body}</p>
          <p className="ae-subtle">{hint}</p>
        </>
      }
      confirmLabel={t("strategy.leverageConfirm.confirm")}
      danger
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
