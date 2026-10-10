import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import "./Chip.css";

export type ChipTone =
  | "neutral"
  | "paper"
  | "live"
  | "accent"
  | "success"
  | "warn"
  | "danger"
  | "running"
  | "waiting"
  | "paused"
  | "stopped"
  | "error";

interface ChipProps {
  tone?: ChipTone;
  title?: string;
  children: ReactNode;
}

/** A small status label. Colour always comes from a semantic token. */
export function Chip({ tone = "neutral", title, children }: ChipProps) {
  return (
    <span className="ae-chip" data-tone={tone} title={title}>
      {children}
    </span>
  );
}

/** "Paper": simulated fills only. */
export function PaperChip() {
  const { t } = useTranslation();
  return (
    <Chip tone="paper" title={t("common.simulatedBadgeTooltip")}>
      {t("states.paper")}
    </Chip>
  );
}

/** "Untested": no replay or forward test exists for what this bot does. */
export function UntestedChip({ title }: { title?: string }) {
  const { t } = useTranslation();
  return (
    <Chip tone="warn" title={title}>
      {t("bots.untested")}
    </Chip>
  );
}

/** "Manual": opened by hand on one signal (Execute), not by the bot's loop. */
export function ManualChip() {
  const { t } = useTranslation();
  return (
    <Chip tone="accent" title={t("bots.manualBadgeTooltip")}>
      {t("bots.manualBadge")}
    </Chip>
  );
}

/** "LIVE": real orders. The word stays untranslated on purpose. */
export function LiveChip() {
  const { t } = useTranslation();
  return (
    <Chip tone="live" title={t("common.realBadgeTooltip")}>
      LIVE
    </Chip>
  );
}

export type DotTone = "success" | "warn" | "danger" | "muted";

/** A coloured status dot (sidebar badges, status bar). */
export function Dot({ tone, label }: { tone: DotTone; label?: string }) {
  return (
    <span
      className="ae-statusdot"
      data-tone={tone}
      role={label ? "img" : undefined}
      aria-label={label}
      title={label}
    />
  );
}
