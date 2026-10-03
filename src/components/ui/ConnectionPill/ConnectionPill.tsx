import type { ReactNode } from "react";
import { Tooltip, type TooltipAlign, type TooltipPlacement } from "@/components/ui/Tooltip/Tooltip";
import "./ConnectionPill.css";

export type ConnectionState = "ok" | "warn" | "down" | "idle" | "checking";

interface ConnectionPillProps {
  /** Short name: "Sentinel", "Binance", "Exchange". */
  name: string;
  state: ConnectionState;
  /** ONE short metric or state word: "74 ms", "no key", "2 min". Never a sentence. */
  metric?: string;
  /** Full detail for the tooltip (FactList rows, the last check's result). */
  detail?: ReactNode;
  /** Accessible name; defaults to "name · metric". */
  ariaLabel?: string;
  /** Click runs an on-demand check. Without it the pill is a readout. */
  onClick?: () => void;
  /** The check cannot run (no key). Stays focusable so the tooltip still explains. */
  unavailable?: boolean;
  tooltipPlacement?: TooltipPlacement;
  tooltipAlign?: TooltipAlign;
}

/**
 * Status dot + short label + one metric; everything else in the tooltip.
 * Never truncates: the text is bounded by the caller (name + one metric).
 *
 *   <ConnectionPill name="Sentinel" state="ok" metric="74 ms" detail={<FactList …/>} onClick={probe} />
 *   <ConnectionPill name="Exchange" state="idle" metric="no key" unavailable />
 *
 * States: ok = green, warn = amber, down = red, idle = grey (not set up /
 * not checked), checking = amber pulse (reduced motion: steady).
 */
export function ConnectionPill({
  name,
  state,
  metric,
  detail,
  ariaLabel,
  onClick,
  unavailable,
  tooltipPlacement = "bottom",
  tooltipAlign = "end",
}: ConnectionPillProps) {
  const body = (
    <>
      <span className="ae-conn__dot" data-state={state} aria-hidden="true" />
      <span className="ae-conn__name">{name}</span>
      {metric ? (
        <>
          <span className="ae-conn__sep" aria-hidden="true">
            ·
          </span>
          <span className="ae-conn__metric tabular">{metric}</span>
        </>
      ) : null}
    </>
  );
  const label = ariaLabel ?? (metric ? `${name} · ${metric}` : name);
  const pill = onClick ? (
    <button
      type="button"
      className="ae-conn"
      data-state={state}
      data-interactive
      aria-label={label}
      aria-disabled={unavailable || state === "checking" || undefined}
      aria-busy={state === "checking" || undefined}
      onClick={() => {
        if (!unavailable && state !== "checking") onClick();
      }}
    >
      {body}
    </button>
  ) : (
    <span className="ae-conn" data-state={state} role="status" aria-label={label} tabIndex={0}>
      {body}
    </span>
  );
  return (
    <Tooltip content={detail} placement={tooltipPlacement} align={tooltipAlign}>
      {pill}
    </Tooltip>
  );
}
