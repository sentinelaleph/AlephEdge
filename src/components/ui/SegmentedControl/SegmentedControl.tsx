import { useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { balancedColumns } from "@/components/ui/KpiGrid/balance";
import "./SegmentedControl.css";

export interface SegmentOption<T extends string> {
  value: T;
  label: ReactNode;
  /** A colour token for the small indicator dot, e.g. "var(--risk-calm)". */
  indicator?: string;
  /** Small secondary text under/next to the label ("+4.4%"). */
  sub?: ReactNode;
  disabled?: boolean;
  /** Why the option is disabled (title). */
  reason?: string;
}

interface SegmentedControlProps<T extends string> {
  /** Accessible name of the radio group. */
  label: string;
  value: T | null;
  options: SegmentOption<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
  size?: "sm" | "md";
  /** Stretch the options to the full width (forms) instead of hugging content. */
  fill?: boolean;
  /**
   * When the options do not fit one row, wrap them into balanced rows of
   * equal cells (5 -> 3 + 2, 6 -> 3 + 3, 4 -> 2 + 2) instead of truncating.
   * Use it wherever the control sits in a panel or a phone-width column.
   */
  wrap?: boolean;
}

/** Content width of one option: padding + its parts, never the cell it fills. */
function naturalWidth(opt: HTMLElement): number {
  const cs = getComputedStyle(opt);
  const parts = Array.from(opt.children) as HTMLElement[];
  const gap = parseFloat(cs.columnGap) || 0;
  const inner = parts.reduce((sum, p) => sum + p.scrollWidth, 0) + gap * Math.max(0, parts.length - 1);
  return Math.ceil(inner + parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight)) + 1;
}

/**
 * The one segmented control: a radio group whose selected option is a raised
 * surface with full-contrast text. Colour is never the label: a level or tone
 * shows as a small dot (`indicator`) next to readable text.
 *
 *   <SegmentedControl label={t("table.market")} value={market} onChange={setMarket} fill
 *     options={[{ value: "spot", label: t("filters.market.spot") }, …]} />
 *   <SegmentedControl label={t("risk.title")} value={level} onChange={pick}
 *     options={RISK_LEVELS.map((l) => ({ value: l, label: t(`risk.levels.${l}`), indicator: `var(--risk-${l})` }))} />
 *
 * Keyboard: arrows move and select (roving tabindex), Home/End jump.
 */
export function SegmentedControl<T extends string>({
  label,
  value,
  options,
  onChange,
  disabled,
  size = "md",
  fill,
  wrap,
}: SegmentedControlProps<T>) {
  const ref = useRef<HTMLDivElement>(null);
  // Wrapped: the column count of the balanced grid; null = one row.
  const [cols, setCols] = useState<number | null>(null);

  useLayoutEffect(() => {
    if (!wrap) return;
    const el = ref.current;
    const host = el?.parentElement;
    if (!el || !host) return;
    const measure = () => {
      const opts = Array.from(el.querySelectorAll<HTMLElement>(".ae-segctl__opt"));
      if (opts.length === 0) return;
      const widths = opts.map(naturalWidth);
      const cs = getComputedStyle(el);
      const hs = getComputedStyle(host);
      const gap = parseFloat(cs.columnGap) || 0;
      const frame =
        parseFloat(cs.paddingLeft) + parseFloat(cs.paddingRight) + parseFloat(cs.borderLeftWidth) + parseFloat(cs.borderRightWidth);
      const room = host.clientWidth - parseFloat(hs.paddingLeft) - parseFloat(hs.paddingRight) - frame;
      const row = widths.reduce((a, b) => a + b, 0) + gap * (widths.length - 1);
      const next = row <= room ? null : balancedColumns(widths.length, room, Math.max(...widths), gap);
      setCols(next !== null && next >= widths.length ? null : next);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(host);
    return () => ro.disconnect();
  });
  const enabled = options.filter((o) => !o.disabled && !disabled);
  const current = options.findIndex((o) => o.value === value);
  const tabStop = current >= 0 && !options[current].disabled ? options[current].value : enabled[0]?.value;

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    if (enabled.length === 0) return;
    const at = Math.max(0, enabled.findIndex((o) => o.value === value));
    let next: number | null = null;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (at + 1) % enabled.length;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (at - 1 + enabled.length) % enabled.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = enabled.length - 1;
    if (next === null) return;
    e.preventDefault();
    const v = enabled[next].value;
    onChange(v);
    requestAnimationFrame(() => ref.current?.querySelector<HTMLButtonElement>(`[data-value="${v}"]`)?.focus());
  };

  return (
    <div
      ref={ref}
      className="ae-segctl"
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled || undefined}
      data-size={size}
      data-fill={fill || undefined}
      data-wrapped={cols !== null || undefined}
      style={cols !== null ? { gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` } : undefined}
      onKeyDown={onKey}
    >
      {options.map((o) => {
        const selected = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={selected}
            data-value={o.value}
            data-selected={selected || undefined}
            tabIndex={o.value === tabStop ? 0 : -1}
            disabled={disabled || o.disabled}
            title={o.disabled ? o.reason : undefined}
            className="ae-segctl__opt"
            onClick={() => onChange(o.value)}
          >
            {o.indicator ? (
              <span className="ae-segctl__dot" style={{ background: o.indicator }} aria-hidden="true" />
            ) : null}
            <span className="ae-segctl__label">{o.label}</span>
            {o.sub ? <span className="ae-segctl__sub tabular">{o.sub}</span> : null}
          </button>
        );
      })}
    </div>
  );
}
