import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Button } from "@/components/ui/Button/Button";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import type { Fact } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { formatUsdt } from "@/lib/format";
import { localeForLanguage } from "@/i18n";
import { botMode, type BotConfig, type TradeMode } from "@/lib/ipc/bot/bot";
import { errorMessage } from "@/lib/ipc/bridge";
import { pineDefaultConfig } from "@/lib/ipc/pine/pine";
import { liveWordKey, ordersOnTestnet, pineTermsChanged, signalStartAction } from "@/lib/liveMode";
import { PositionsTable } from "@/pages/Positions/PositionsTable";
import { ModePanel } from "@/pages/Bots/signal/SignalBotForm";
import { useSignalLive } from "@/pages/Bots/signal/useSignalLive";
import "@/pages/Bots/strategy/StrategyForm.css";
import "@/pages/Bots/bots.css";
import "./pine.css";
import { configFromForm, formError, formFromConfig, mappedTriggers, shortHash, type PineBotForm } from "./pineBotModel";
import { inputInfos, inputProblem, inputValue, numericTitles } from "./pineInputs";
import { triggerLabel } from "./pineText";
import { PineMapPanel } from "./PineMapPanel";
import { PineResults } from "./PineResults";
import { PineScriptPanel } from "./PineScriptPanel";
import { PineSettingsPanels } from "./PineSettingsPanels";
import { PineStatusPanel } from "./PineStatusPanel";
import { usePineCompile, usePineDesk } from "./usePine";

/**
 * #/bots/pine: the Pine bot, which runs the user's own Pine-lite script on
 * closed Binance USDT-M bars and acts on the triggers the user mapped. Paper
 * by default; Live with the typed LIVE every session, a bot-level stop
 * required. Rust checks every value on save and every action on each bar;
 * this form only writes them. The script stays on this computer.
 */
export function PineBotPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk } = useDeskContext();
  const s = desk.status;
  const cfg = desk.loaded ? (s.pine ?? null) : null;
  const running = s.pineRunning === true;
  const maxLeverage = risk.state?.limits.maxLeverage ?? null;
  const [form, setForm] = useState<PineBotForm>(() => formFromConfig(cfg));
  const [chosen, setChosen] = useState<TradeMode>(botMode(cfg));
  const [bad, setBad] = useState<Record<string, boolean>>({});
  const [base, setBase] = useState<BotConfig | null>(cfg);
  const [error, setError] = useState<string | null>(null);
  const k = (x: string) => `botsPine.${x}`;
  const compile = usePineCompile(form.script, t(k("compileFailed")));
  const shown = compile.info?.ok ? compile.info : compile.last;
  const inputs = shown ? inputInfos(shown.inputs) : [];
  const card = usePineDesk(cfg !== null);

  const liveExtra = (saved: BotConfig | null): Fact[] => {
    const p = saved?.pine;
    if (!p) return [];
    const mapped = mappedTriggers(p, shown?.triggers ?? null);
    const pct = (v: number | null, from: string | null) =>
      [v !== null ? `${v}%` : null, from ? t(k("fromInputShort"), { input: from }) : null].filter(Boolean).join(" · ") || t(k("off"));
    return [
      { label: t(k("hash")), value: <span className="mono">{shortHash(p.scriptHash)}</span> },
      {
        label: t(k("triggersMapped")),
        value: mapped.length ? mapped.map((m) => `${triggerLabel(t, m)} → ${t(k(`action.${m.action}`))}`).join(" · ") : t(k("triggersNoneMapped")),
        tone: mapped.length ? undefined : "warn",
      },
      { label: t(k("market")), value: `${p.symbols.join(", ")} · ${p.timeframe}` },
      { label: t(k("exits")), value: `${t(k("stop"))} ${pct(p.stopPct, p.stopInput)} · ${t(k("target"))} ${pct(p.takeProfitPct, p.targetInput)}` },
      { label: t(k("budget")), value: `${formatUsdt(p.budgetUsdt, locale)} · ${t(k("maxSize"))} ${formatUsdt(p.maxSizeUsdt, locale)}` },
    ];
  };
  const live = useSignalLive("pine", liveExtra);

  // The stored bot (or the server's defaults for a new one) is the base;
  // re-seeded only when the stored bot itself changes.
  const cfgKey = JSON.stringify(cfg);
  useEffect(() => {
    if (cfg) {
      setBase(cfg);
      setForm(formFromConfig(cfg));
      setChosen(botMode(cfg));
      return;
    }
    if (!desk.loaded) return;
    let gone = false;
    pineDefaultConfig()
      .then((d) => !gone && setBase(d))
      .catch((e) => !gone && setError(errorMessage(e, t("bots.defaultsFailed"))));
    return () => {
      gone = true;
    };
  }, [desk.loaded, cfgKey]);

  const set = (patch: Partial<PineBotForm>) => setForm((f) => ({ ...f, ...patch }));
  const invalid = (key: string) => (v: boolean) => setBad((b) => ({ ...b, [key]: v }));
  const inputsBad = inputs.some((i) => inputProblem(i, inputValue(i, form.inputs)) !== null);
  const formErr = formError(form, compile.info, maxLeverage, chosen === "live");
  const anyBad = Object.values(bad).some(Boolean) || inputsBad;
  const next = base ? configFromForm(form, base, compile.info) : null;
  const action = signalStartAction(cfg, chosen, next);
  const canSave = !!next && !formErr && !anyBad && !desk.busy;
  // Why Save and Start are off, said next to them: the field rule's own message
  // also sits under the settings, far below the buttons.
  const formErrText = formErr ? t(k(`error.${formErr.key}`), formErr.params) : null;
  const blockedWhy = !running && formErrText ? t(k("blocked"), { reason: formErrText }) : undefined;
  // Saving these edits on an armed bot disarms and stops it (Rust normalize_config).
  const disarmsOnSave = cfg?.live === true && !!next && pineTermsChanged(cfg.pine, next.pine);
  const waitingArm = cfg !== null && botMode(cfg) === "live" && cfg.live !== true;

  const save = async () => {
    if (!next) return;
    setError(null);
    const err = await desk.configure(next);
    if (err) setError(err);
  };

  const start = async () => {
    if (!next) return;
    setError(null);
    if (action === "confirmLive") return live.control.onStartLive(next);
    await desk.configureAndStart(next);
  };

  const mine = s.openPositions.filter((p) => p.botKind === "pine");
  const right = (
    <PineStatusPanel
      cfg={cfg}
      status={card.status}
      running={running}
      pilotLeft={s.pinePilotLeft ?? 0}
      marginUsed={mine.reduce((n, p) => n + p.capital, 0)}
      unsaved={cfg !== null && !!next && pineTermsChanged(cfg.pine, next.pine)}
    />
  );

  const primary = running ? (
    <Button size="sm" variant="secondary" disabled={desk.busy} onClick={() => void desk.stop("pine")}>
      {t("bots.stop")}
    </Button>
  ) : (
    <Button size="sm" variant={chosen === "live" ? "danger" : "primary"} disabled={!canSave || s.killSwitchTripped} disabledReason={formErrText} tooltipPlacement="bottom" onClick={() => void start()}>
      {chosen === "live" ? t(liveWordKey("startLive", ordersOnTestnet(s, desk.loaded))) : t("bots.start")}
    </Button>
  );

  return (
    <PageShell
      title={t("page.pineBot")}
      crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]}
      right={right}
      primary={primary}
      secondary={
        <Button size="sm" variant="secondary" disabled={!canSave} disabledReason={formErrText} tooltipPlacement="bottom" onClick={() => void save()}>
          {t("botsTv.save")}
        </Button>
      }
    >
      <ModePanel config={cfg} chosen={chosen} onChoose={setChosen} busy={desk.busy} live={live.control} />
      {live.dialog}
      {blockedWhy ? (
        <p className="ae-banner" data-tone="warn" role="status" data-testid="pine-blocked">
          {blockedWhy}
        </p>
      ) : null}
      {waitingArm ? (
        <p className="ae-banner" data-tone="warn" role="status">
          {t(k("waitingArm"))}
        </p>
      ) : null}
      {disarmsOnSave ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {t(k("disarmsOnSave"))}
        </p>
      ) : null}

      {mine.length > 0 ? (
        <Section title={t("table.openPositions")} count={mine.length}>
          <PositionsTable positions={mine} emptyKey="positions.noOpen" onClose={desk.closePosition} />
        </Section>
      ) : null}

      <PineScriptPanel
        name={form.name}
        script={form.script}
        compile={compile}
        savedHash={cfg?.pine?.scriptHash || null}
        onName={(v) => set({ name: v })}
        onScript={(v) => set({ script: v })}
      />
      <PineMapPanel
        inputs={inputs}
        overrides={form.inputs}
        triggers={shown?.triggers ?? []}
        map={form.triggers}
        onInput={(id, v) => set({ inputs: { ...form.inputs, [String(id)]: v } })}
        onMap={(id, a) => set({ triggers: { ...form.triggers, [id]: a } })}
        mapError={formErr?.key === "triggersNone" ? formErrText : null}
      />
      <PineSettingsPanels form={form} set={set} invalid={invalid} maxLeverage={maxLeverage} numericInputs={numericTitles(inputs)} live={chosen === "live"} />

      {formErr ? (
        <p className="ae-sfield__error" role="alert">
          {formErrText}
        </p>
      ) : null}
      {error ? (
        <p className="ae-sfield__error" role="alert">
          {error}
        </p>
      ) : null}

      <PineResults rows={card.results} error={card.error} />
    </PageShell>
  );
}
