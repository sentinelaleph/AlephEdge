import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "@/app/router/router";
import type { BotDeskController } from "@/components/BotDesk/useBotDesk";
import "./FirstRunChecklist.css";

interface FirstRunChecklistProps {
  desk: BotDeskController;
  /** The Sentinel stream state from the desk's single health/stream poller. */
  streamConnected: boolean;
  /** Shown once every step is done: hides the checklist (the caller persists it). */
  onDismiss?: () => void;
}

interface Step {
  key: "signedIn" | "streamConnected" | "botConfigured" | "botRunning";
  done: boolean;
  hint?: string;
  /** The page that completes this step. */
  to?: string;
}

/**
 * "What do I do next?" — derived entirely from real state, never a static
 * onboarding graphic. The order matches the actual gate order a new user
 * has to clear: sign in (already true by the time this renders — Workspace
 * gates everything before it) → Sentinel stream connects → configure a bot →
 * start it. Collapses to a one-line summary once every step is done, but
 * never disappears — the live-trading-locked note stays visible either way,
 * because paper vs. real money is not a thing a user should have to
 * remember after the checklist scrolls away.
 */
export function FirstRunChecklist({ desk, streamConnected, onDismiss }: FirstRunChecklistProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(true);

  const s = desk.status;
  const botConfigured = s.futures !== null || s.spot !== null || s.pump !== null;
  const botRunning = s.futuresRunning || s.spotRunning || s.pumpRunning;

  const steps: Step[] = [
    { key: "signedIn", done: true },
    {
      key: "streamConnected",
      done: streamConnected,
      hint: streamConnected ? undefined : t("checklist.steps.streamWaiting"),
      to: "/signals",
    },
    {
      key: "botConfigured",
      done: botConfigured,
      hint: botConfigured ? undefined : t("checklist.steps.botConfiguredOnCard"),
      to: "/bots/signal",
    },
    {
      key: "botRunning",
      done: botRunning,
      hint: botRunning ? undefined : t("checklist.steps.botRunningHint"),
      to: "/bots/signal",
    },
  ];

  const doneCount = steps.filter((st) => st.done).length;
  const allDone = doneCount === steps.length;

  return (
    <section className="ae-checklist" data-done={allDone}>
      <button
        type="button"
        className="ae-checklist__bar"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        <span className="ae-checklist__title">
          {allDone ? t("checklist.allDone") : t("checklist.title")}
        </span>
        {/* Denominator always shown next to the count — a bare "2 done" reads
            as an achievement; "2 of 4" reads as what's left. */}
        <span className="ae-checklist__progress">
          {t("checklist.progress", { done: doneCount, total: steps.length })}
        </span>
        <span className="ae-checklist__chevron" aria-hidden="true">
          {expanded ? "▾" : "▸"}
        </span>
      </button>

      {expanded ? (
        <>
          <ol className="ae-checklist__steps">
            {steps.map((st) => (
              <li key={st.key} className="ae-checklist__step" data-done={st.done}>
                <span className="ae-checklist__mark" aria-hidden="true">
                  {st.done ? "✓" : ""}
                </span>
                <span className="ae-checklist__body">
                  {st.to && !st.done ? (
                    <Link to={st.to} className="ae-checklist__label ae-checklist__link">
                      {t(`checklist.steps.${st.key}`)}
                    </Link>
                  ) : (
                    <span className="ae-checklist__label">{t(`checklist.steps.${st.key}`)}</span>
                  )}
                  {st.hint ? <span className="ae-checklist__hint">{st.hint}</span> : null}
                </span>
              </li>
            ))}
          </ol>
          {/* Only once the desk has reported which build this is: the
              placeholder status would claim "locked" in a live build. */}
          {desk.loaded ? (
            <p className="ae-checklist__note">
              {t(s.liveTradingEnabled ? "checklist.liveAvailableNote" : "checklist.liveLockedNote")}
            </p>
          ) : null}
          {allDone && onDismiss ? (
            <button type="button" className="ae-checklist__dismiss" onClick={onDismiss}>
              {t("dashboard.dismissChecklist")}
            </button>
          ) : null}
        </>
      ) : null}
    </section>
  );
}
