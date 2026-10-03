import type { ReactNode } from "react";
import { Tooltip, type TooltipAlign, type TooltipPlacement } from "@/components/ui/Tooltip/Tooltip";
import "./InfoTip.css";

interface InfoTipProps {
  /** Accessible name of the icon button (what the tip explains). */
  label: string;
  content: ReactNode;
  placement?: TooltipPlacement;
  align?: TooltipAlign;
  /** 16px inside a field label or a dense row; 20px next to a line of text. */
  size?: "sm" | "md";
}

/**
 * A small "i" next to a label that holds the explanatory text on hover and
 * focus, so the screen keeps labels and numbers only.
 *
 *   <InfoTip label={t("backtest.label.historical")} content={t("strategy.preset.simulationLabel")} />
 *   <TextField label={t("risk.dailyLoss")} info={t("risk.dailyLossHint", { cap })} … />
 */
export function InfoTip({ label, content, placement = "bottom", align = "start", size = "md" }: InfoTipProps) {
  const px = size === "sm" ? 12 : 14;
  return (
    <Tooltip content={content} placement={placement} align={align}>
      <button type="button" className="ae-infotip" data-size={size} aria-label={label}>
        <svg width={px} height={px} viewBox="0 0 24 24" aria-hidden="true" focusable="false">
          <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeWidth="1.8" />
          <path d="M12 11v6M12 7.5v.01" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
        </svg>
      </button>
    </Tooltip>
  );
}
