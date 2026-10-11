import { useEffect, useState, type ReactNode } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { navigate } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { errorMessage } from "@/lib/ipc/bridge";
import { strategyPresets, type Preset } from "@/lib/ipc/strategy/strategy";
import { presetName, strategyErrorText } from "@/lib/strategyText";

/** "1 Nov" / "1 Kas" from an ISO date; "October 2026" from "2026-10". */
export function shortDate(iso: string, locale: string): string {
  const d = new Date(`${iso}T00:00:00Z`);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(locale, { day: "numeric", month: "short", timeZone: "UTC" });
}
export function monthName(ym: string, locale: string): string {
  const d = new Date(`${ym}-01T00:00:00Z`);
  return Number.isNaN(d.getTime()) ? ym : d.toLocaleDateString(locale, { month: "long", year: "numeric", timeZone: "UTC" });
}

/** The engine's templates (strategy_presets); null while loading. */
export function usePresets() {
  const { t } = useTranslation();
  const [presets, setPresets] = useState<Preset[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    strategyPresets()
      .then((p) => alive && setPresets(p))
      .catch((e: unknown) => alive && setError(strategyErrorText(t, errorMessage(e, "storeReadFailed"))));
    return () => {
      alive = false;
    };
  }, [t]);
  return { presets, error };
}

/** Where a template stands: passed, passed to a date and waiting for a re-read, or failed. */
export type VerdictBucket = "passed" | "review" | "failed";

export function verdictBucket(p: Preset): VerdictBucket {
  if (p.verdict !== "presetReady") return "failed";
  return p.history.recheck ? "review" : "passed";
}

export function testSplit(p: Preset) {
  return p.history.splits.find((s) => s.split === "test") ?? null;
}

export function VerdictChip({ p }: { p: Preset }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  if (p.verdict === "presetReady" && p.history.recheck) {
    const r = p.history.recheck;
    return (
      <StatusChip
        status="underReview"
        label={t("strategy.preset.verdicts.underReview", { end: shortDate(r.testWindowEnd, locale), date: shortDate(r.nextRead, locale) })}
      />
    );
  }
  return p.verdict === "presetReady" ? (
    <StatusChip status="ok" label={t("strategy.preset.verdicts.presetReady")} />
  ) : (
    <StatusChip status="error" label={t("strategy.preset.verdicts.failed")} />
  );
}

/** Why a failed template failed, one line per check. */
export function reasonLines(t: TFunction, p: Preset): ReactNode {
  return (
    <ul className="ae-list">
      {p.failReasons.map((r) => (
        <li key={r}>{t(`strategy.preset.reason.${r}`, { defaultValue: r })}</li>
      ))}
    </ul>
  );
}

/** The create form for a template, on its own pair or on `symbol`. */
export function presetFormPath(p: Preset, symbol?: string | null): string {
  return withQuery(`/bots/${p.config.params.kind}/new`, { preset: p.id, symbol: symbol ?? null });
}

/**
 * "Use preset" for a template that failed its checks asks first, with the
 * checks it failed; a passed one opens the form directly. `symbol` opens the
 * form on that pair; `beforeGo` runs right before the navigation.
 */
export function useTemplateLauncher(opts: { symbol?: string | null; beforeGo?: () => void } = {}) {
  const { t } = useTranslation();
  const [held, setHeld] = useState<Preset | null>(null);
  const go = (p: Preset) => {
    opts.beforeGo?.();
    navigate(presetFormPath(p, opts.symbol));
  };
  const launch = (p: Preset) => (p.verdict === "presetReady" ? go(p) : setHeld(p));
  const dialog = (
    <ConfirmDialog
      open={held !== null}
      title={t("strategy.preset.failedConfirm.title", { name: held ? presetName(t, held) : "" })}
      body={
        held ? (
          <>
            <p>{t("strategy.preset.failedConfirm.body")}</p>
            {reasonLines(t, held)}
            <p className="ae-subtle">{t("strategy.preset.failedConfirm.hint")}</p>
          </>
        ) : null
      }
      confirmLabel={t("strategy.preset.failedConfirm.confirm")}
      danger
      onCancel={() => setHeld(null)}
      onConfirm={() => {
        const p = held;
        setHeld(null);
        if (p) go(p);
      }}
    />
  );
  return { launch, dialog, holding: held !== null };
}
