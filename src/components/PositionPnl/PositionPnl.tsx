import type { TFunction } from "i18next";
import type { DataColumn } from "@/components/ui/DataTable/DataTable";
import { formatPnl, formatPrice, formatSignedPercent, formatTableTime, NO_VALUE, pnlToneAttr } from "@/lib/format";
import type { OpenPosition, PositionMark } from "@/lib/ipc/bot/bot";
import { MARK_STALE_MS, markKey, rangePosition } from "@/lib/positionMarks";
import "./PositionPnl.css";

/**
 * Where the price sits between the stop (left) and the target (right), with
 * the entry as a tick: whether a position is ahead or behind reads at once.
 */
export function RangeBar({ position, price, label }: { position: OpenPosition; price: number; label: string }) {
  const r = rangePosition(position, price);
  if (!r) return null;
  const tone = r.price >= r.entry ? "up" : "down";
  return (
    <span className="ae-range" role="img" aria-label={label} title={label}>
      <span className="ae-range__track" />
      <span
        className="ae-range__fill"
        data-tone={tone}
        style={{ left: `${Math.min(r.entry, r.price) * 100}%`, width: `${Math.abs(r.price - r.entry) * 100}%` }}
      />
      <span className="ae-range__entry" style={{ left: `${r.entry * 100}%` }} />
      <span className="ae-range__dot" data-tone={tone} style={{ left: `${r.price * 100}%` }} />
    </span>
  );
}

/**
 * The three columns every open-position table shares: the last engine price,
 * the P&L if closed there (USDT and % of the position's capital, fees
 * included), and the stop-to-target bar. A position the engine has not priced
 * yet shows a dash, never an estimate.
 */
export function positionPnlColumns(
  t: TFunction,
  locale: string,
  marks: Map<string, PositionMark>,
  now: number,
): DataColumn<OpenPosition>[] {
  const markOf = (p: OpenPosition) => marks.get(markKey(p.botKind, p.signalId));
  const stale = (m: PositionMark) => now - m.atMs > MARK_STALE_MS;
  return [
    {
      id: "price",
      header: t("positions.mark.price"),
      headerTip: t("positions.mark.priceTip"),
      numeric: true,
      priority: 2,
      cell: (p) => {
        const m = markOf(p);
        if (!m) return NO_VALUE;
        return (
          <span className={stale(m) ? "ae-mark--stale" : undefined} title={t("positions.mark.at", { time: formatTableTime(m.atMs, locale, now) })}>
            {formatPrice(m.price, locale)}
          </span>
        );
      },
    },
    {
      id: "pnlNow",
      header: t("positions.mark.pnl"),
      headerTip: t("positions.mark.pnlTip"),
      numeric: true,
      keep: true,
      cell: (p) => {
        const m = markOf(p);
        if (!m) return <span className="ae-subtle">{t("positions.mark.pending")}</span>;
        return (
          <span className="ae-pnlnow">
            <span>{formatPnl(m.pnlUsdt, locale, { unit: "none" }).text}</span>
            <span className="ae-pnlnow__pct">{formatSignedPercent(m.pnlPct, locale)}</span>
          </span>
        );
      },
      tone: (p) => {
        const m = markOf(p);
        return m ? pnlToneAttr(m.pnlUsdt) : "muted";
      },
    },
    {
      id: "range",
      header: t("positions.mark.range"),
      headerTip: t("positions.mark.rangeTip"),
      priority: 2,
      width: "96px",
      cell: (p) => {
        const m = markOf(p);
        if (!m) return null;
        const r = rangePosition(p, m.price);
        if (!r) return null;
        return <RangeBar position={p} price={m.price} label={t("positions.mark.rangeLabel", { pct: Math.round(r.price * 100) })} />;
      },
    },
  ];
}
