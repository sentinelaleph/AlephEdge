/**
 * Bar-close equity over time, budget as the dashed base line. Inline SVG,
 * shared by the bot detail and the backtest report (styles in StrategyForm.css).
 */
export function EquityChart({ rows, budget, label }: { rows: { ts: number; equity: number }[]; budget: number; label: string }) {
  if (rows.length < 2) return null;
  const xs = rows.map((r) => r.ts);
  const ys = rows.map((r) => r.equity);
  const x0 = xs[0];
  const x1 = xs[xs.length - 1];
  const lo = Math.min(budget, ...ys);
  const hi = Math.max(budget, ...ys);
  const span = hi - lo || 1;
  const X = (v: number) => ((v - x0) / (x1 - x0 || 1)) * 100;
  const Y = (v: number) => 100 - ((v - lo) / span) * 100;
  const pts = rows.map((r) => `${X(r.ts).toFixed(3)},${Y(r.equity).toFixed(3)}`).join(" ");
  return (
    <svg className="ae-equity" viewBox="0 0 100 100" preserveAspectRatio="none" role="img" aria-label={label}>
      <line className="ae-equity__base" x1="0" x2="100" y1={Y(budget)} y2={Y(budget)} />
      <polyline className="ae-equity__line" points={pts} />
    </svg>
  );
}
