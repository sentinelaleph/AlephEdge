import { Children, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { balancedColumns } from "./balance";
import "./KpiGrid.css";

interface KpiGridProps {
  /** KpiTile elements. */
  children: ReactNode;
  /** Narrowest a tile may get (CSS px). Default 160. */
  minTile?: number;
  /** Accessible group name, when the tiles need one. */
  label?: string;
}

const GAP = 8;

/**
 * A row of KpiTiles: equal heights per row, and the columns balanced to the
 * tile count so no row ends with a lonely tile (6 tiles: 3 + 3, not 4 + 2).
 * Falls back to CSS auto-fit before the first measurement.
 *
 *   <KpiGrid>
 *     <KpiTile label={…} value={…} />
 *     …
 *   </KpiGrid>
 */
export function KpiGrid({ children, minTile = 160, label }: KpiGridProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState<number | null>(null);
  const count = Children.toArray(children).filter(Boolean).length;

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(el.clientWidth);
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width;
      if (w !== undefined) setWidth(Math.round(w));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const cols = width === null ? null : balancedColumns(count, width, minTile, GAP);
  const style = {
    "--kpi-min": `${minTile}px`,
    ...(cols ? { gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))` } : {}),
  } as CSSProperties;

  return (
    <div ref={ref} className="ae-kpigrid" style={style} role={label ? "group" : undefined} aria-label={label}>
      {children}
    </div>
  );
}
