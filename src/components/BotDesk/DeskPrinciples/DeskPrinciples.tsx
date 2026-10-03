import { useState } from "react";
import { useTranslation } from "react-i18next";
import "./DeskPrinciples.css";

/**
 * "How this bot works" — the collapsible explainer under the bot desk.
 *
 * Every line here is a property the CODE actually has, not positioning copy:
 * the refusal feed exists (engine pushes a visible skip for every non-trade,
 * PRD §5.4), fills are simulated at the live venue price with taker fees
 * (engine/mod.rs), an invalidated signal is simply never entered — an
 * already-open position is kept and left to its stop/breakeven/target,
 * because closing on invalidation was measured to lose money (engine/mod.rs),
 * keys sit in the encrypted local vault (vault/mod.rs), and live orders are
 * double-gated off with an exchange-side protective stop when they do go
 * live (engine/live.rs). If a claim stops being true in code, it must be
 * deleted here — this panel is a contract, not an ad.
 */
const PRINCIPLE_KEYS = [
  "evidence",
  "noSilentSkips",
  "honestFills",
  "veto",
  "riskFirst",
  "localFirst",
] as const;

export function DeskPrinciples() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  return (
    <section className="ae-principles" data-open={open || undefined}>
      <button
        type="button"
        className="ae-principles__toggle"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
      >
        <span>{t("bots.principles.title")}</span>
        <span className="ae-principles__chevron" aria-hidden>
          {open ? "−" : "+"}
        </span>
      </button>

      {open ? (
        <div className="ae-principles__body">
          <p className="ae-principles__intro">{t("bots.principles.intro")}</p>
          <ul className="ae-principles__list">
            {PRINCIPLE_KEYS.map((key) => (
              <li key={key} className="ae-principles__item">
                <h4>{t(`bots.principles.items.${key}.title`)}</h4>
                <p>{t(`bots.principles.items.${key}.body`)}</p>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}
