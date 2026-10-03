import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice } from "@/lib/format";
import type { PreviewDto, StrategyConfig } from "@/lib/ipc/strategy/strategy";

/**
 * A leveraged DCA with no stop loss: once price runs past the last safety
 * order, nothing closes the deal before the exchange liquidates it and the
 * cycle's whole margin is gone. Owner's backtest 3 Oct: 5x, no stop, one
 * cycle at -100% of budget. Such a config is allowed, but only after the
 * user has read the liquidation distance and confirmed it.
 */
export function needsLeverageConfirm(cfg: StrategyConfig): boolean {
  return cfg.params.kind === "dca" && cfg.leverage > 1 && cfg.params.slPct === null;
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
  const dca = preview?.dca ?? null;
  const pct = dca?.liqDistancePct;
  const liq = dca?.liqPrice;
  const side = cfg.side === "short" ? "short" : "long";
  const body =
    pct != null && liq != null
      ? t(`strategy.leverageConfirm.body_${side}`, {
          leverage: cfg.leverage,
          pct: formatNumber(Math.abs(pct), locale, { maximumFractionDigits: 1 }),
          price: formatPrice(liq, locale),
        })
      : t("strategy.leverageConfirm.bodyGeneric", { leverage: cfg.leverage });
  return (
    <ConfirmDialog
      open={open}
      title={t("strategy.leverageConfirm.title")}
      body={
        <>
          <p>{body}</p>
          <p className="ae-subtle">{t("strategy.leverageConfirm.hint")}</p>
        </>
      }
      confirmLabel={t("strategy.leverageConfirm.confirm")}
      danger
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}
