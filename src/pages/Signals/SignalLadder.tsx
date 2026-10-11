import { useTranslation } from "react-i18next";
import { localeForLanguage } from "@/i18n";
import { formatPrice, formatSignedPercent } from "@/lib/format";
import type { Signal } from "@/lib/ipc/signal/signal";
import { ladder } from "@/lib/signalLevels";

/**
 * The signal's levels on one scale, stop on the left and the furthest target
 * on the right: each with its price and its move from the entry, so the
 * room to the stop against the room to each target reads at a glance.
 */
export function SignalLadder({ signal }: { signal: Signal }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const points = ladder(signal);
  if (!points) return null;
  const entry = points.find((p) => p.kind === "entry")!;
  const label = (p: (typeof points)[number]) => (p.kind === "sl" ? t("table.sl") : p.kind === "entry" ? t("table.entry") : `TP${p.n}`);

  return (
    <div className="ae-ladder" role="img" aria-label={t("signalsPage.ladder.label")}>
      <div className="ae-ladder__track">
        <span className="ae-ladder__risk" style={{ left: 0, width: `${entry.at * 100}%` }} />
        <span className="ae-ladder__reward" style={{ left: `${entry.at * 100}%`, width: `${(1 - entry.at) * 100}%` }} />
        {points.map((p) => (
          <span key={`${p.kind}${p.n ?? ""}`} className="ae-ladder__tick" data-kind={p.kind} style={{ left: `${p.at * 100}%` }} />
        ))}
      </div>
      <ul className="ae-ladder__levels">
        {points.map((p) => (
          <li key={`${p.kind}${p.n ?? ""}`} data-kind={p.kind}>
            <span className="ae-ladder__name">{label(p)}</span>
            <span className="ae-ladder__price tabular">{formatPrice(p.price, locale)}</span>
            <span className="ae-ladder__pct tabular" data-tone={p.pct > 0 ? "up" : p.pct < 0 ? "down" : undefined}>
              {p.kind === "entry" ? "" : formatSignedPercent(p.pct, locale)}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
