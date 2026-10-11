import { useTranslation } from "react-i18next";
import { localeForLanguage } from "@/i18n";
import { formatPnl } from "@/lib/format";
import type { DayNet } from "@/lib/tradeStats";

/** Days of bars drawn at most; a longer selection shows its latest days. */
export const BAR_DAYS = 120;

function dateLabel(ms: number, locale: string): string {
  return new Date(ms).toLocaleDateString(locale, { day: "numeric", month: "short", timeZone: "UTC" });
}

/**
 * Net P&L over time: the running total after each closed trade (line, zero
 * as the dashed base) and the net of each UTC day (bars, gains up, losses
 * down). One scale per strip; the labels name its ends.
 */
export function PnlChart({ points, days }: { points: { ts: number; value: number }[]; days: DayNet[] }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  if (points.length < 2) return <p className="ae-subtle">{t("history.chart.tooFew")}</p>;

  const x0 = points[0].ts;
  const x1 = points[points.length - 1].ts;
  const values = points.map((p) => p.value);
  const lo = Math.min(0, ...values);
  const hi = Math.max(0, ...values);
  const span = hi - lo || 1;
  const X = (v: number) => ((v - x0) / (x1 - x0 || 1)) * 100;
  const Y = (v: number) => 100 - ((v - lo) / span) * 100;
  const line = points.map((p) => `${X(p.ts).toFixed(3)},${Y(p.value).toFixed(3)}`).join(" ");
  const last = values[values.length - 1];
  const area = `0,${Y(0).toFixed(3)} ${line} 100,${Y(0).toFixed(3)}`;

  const shown = days.slice(-BAR_DAYS);
  const maxAbs = Math.max(...shown.map((d) => Math.abs(d.net)), 0) || 1;
  const usdt = (n: number) => formatPnl(n, locale).text;

  return (
    <div className="ae-pnlchart">
      <div className="ae-pnlchart__plot">
        <div className="ae-pnlchart__axis">
          <span>{usdt(hi)}</span>
          <span>{usdt(lo)}</span>
        </div>
        <svg
          className="ae-pnlchart__svg"
          viewBox="0 0 100 100"
          preserveAspectRatio="none"
          role="img"
          aria-label={t("history.chart.lineLabel", { value: usdt(last) })}
        >
          <polygon className="ae-pnlchart__area" data-tone={last >= 0 ? "up" : "down"} points={area} />
          <line className="ae-pnlchart__zero" x1="0" x2="100" y1={Y(0)} y2={Y(0)} />
          <polyline className="ae-pnlchart__line" data-tone={last >= 0 ? "up" : "down"} points={line} />
        </svg>
      </div>
      <div className="ae-pnlchart__plot ae-pnlchart__plot--bars">
        <div className="ae-pnlchart__axis">
          <span>{days.length > BAR_DAYS ? t("history.chart.dailyLast", { count: BAR_DAYS }) : t("history.chart.daily")}</span>
        </div>
        <div className="ae-pnlchart__bars" role="img" aria-label={t("history.chart.barsLabel", { count: shown.length })}>
          {shown.map((d) => (
            <span
              key={d.day}
              className="ae-pnlchart__day"
              title={`${dateLabel(d.day, locale)} · ${usdt(d.net)} · ${t("history.chart.tradesN", { count: d.trades })}`}
            >
              {d.net !== 0 ? (
                <span
                  className="ae-pnlchart__bar"
                  data-tone={d.net > 0 ? "up" : "down"}
                  style={{ height: `${Math.max(4, (Math.abs(d.net) / maxAbs) * 50)}%` }}
                />
              ) : null}
            </span>
          ))}
        </div>
      </div>
      <div className="ae-pnlchart__dates">
        <span>{dateLabel(x0, locale)}</span>
        <span>{dateLabel(x1, locale)}</span>
      </div>
    </div>
  );
}
