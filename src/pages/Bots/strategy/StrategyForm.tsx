import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, useNavigationGuard } from "@/app/router/router";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { LeverageRiskDialog, needsLeverageConfirm } from "./LeverageRiskDialog";
import { Button } from "@/components/ui/Button/Button";
import { ReadOnlyField } from "@/components/ui/ReadOnlyField/ReadOnlyField";
import { Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import { exchangeSymbols } from "@/lib/ipc/exchange/exchange";
import {
  parseStrategyError,
  strategyPreview,
  strategyValidate,
  type DcaParams,
  type GridParams,
  type Preset,
  type PreviewDto,
  type StrategyBotView,
  type StrategyConfig,
  type StrategyErrorDto,
  type StrategyKind,
  type StrategySide,
} from "@/lib/ipc/strategy/strategy";
import { strategyErrorText } from "@/lib/strategyText";
import { CheckRow, FieldShell, NumField, Segmented } from "./formControls";
import { StrategyLevelsTable } from "./StrategyPreviewPanel";
import "./StrategyForm.css";

/** Wire-type ceilings of the Rust config (u8 / u16 / u32). Not bounds. */
const U8 = 255;
const U16 = 65_535;
const U32 = 4_294_967_295;

export interface StrategyFormProps {
  kind: StrategyKind;
  /** Rust defaults, a preset's config, a cloned bot's or the edited bot's. */
  initial: StrategyConfig;
  /** Edit mode: the bot being edited (locks what Rust refuses to change). */
  bot?: StrategyBotView;
  /** The preset the form was opened from (parity check). */
  preset?: Preset | null;
  /** Called with the config to save; resolves to a raw error code or null. */
  onSubmit: (cfg: StrategyConfig, startAfter: boolean) => Promise<string | null>;
  onCancel: () => void;
  /** Receives the live preview for the page's summary panel. */
  onPreview: (p: FormPreview) => void;
  onDirty?: (dirty: boolean) => void;
  /**
   * "backtest": the same fields feed a historical simulation. No create
   * buttons, no leave guard, no budget-cap hint, no BTC-break switch (no
   * BTC-break history exists) and no portfolio-breaker switch (the backtest
   * does not simulate it); the page reads the config through `onState`.
   */
  mode?: "bot" | "backtest";
  /** The current config and whether Rust accepts it (backtest mode). */
  onState?: (s: { cfg: StrategyConfig; ok: boolean }) => void;
  /**
   * The page renders Create / Save in its header: the form drops its own
   * action row and reports what the buttons need instead.
   */
  onActions?: (a: FormActions) => void;
}

export interface FormActions {
  canSubmit: boolean;
  saving: boolean;
  /** Stable across renders; runs the current submit. */
  submit: (startAfter: boolean) => void;
}

export interface FormPreview {
  preview: PreviewDto | null;
  error: string | null;
  cfg: StrategyConfig;
  /** Strategy budget free under the cap, this bot's own reservation added back. */
  available: number | null;
}

/** Does `cfg` still describe the preset's historical simulation? */
export function matchesPreset(cfg: StrategyConfig, preset: Preset): boolean {
  const a = preset.config;
  return (
    JSON.stringify(a.params) === JSON.stringify(cfg.params) &&
    a.market === cfg.market &&
    a.side === cfg.side &&
    a.leverage === cfg.leverage &&
    a.maxDrawdownPct === cfg.maxDrawdownPct &&
    a.pauseOnBtcBreak === cfg.pauseOnBtcBreak &&
    // The preset was simulated without the portfolio breaker: switching it on is a deviation.
    a.portfolioBreaker === cfg.portfolioBreaker &&
    JSON.stringify(a.start) === JSON.stringify(cfg.start) &&
    JSON.stringify(a.restart) === JSON.stringify(cfg.restart)
  );
}

/** Minutes to hours for display; hours typed become whole minutes. */
const toHours = (min: number) => Math.round((min / 60) * 100) / 100;
const toMin = (h: number) => Math.max(1, Math.round(h * 60));

function localInput(ms: number): string {
  const d = new Date(ms - new Date(ms).getTimezoneOffset() * 60_000);
  return d.toISOString().slice(0, 16);
}

/**
 * The DCA / Grid create and edit form. Every field maps 1:1 onto Rust's
 * StrategyConfig; Rust validates on each change (strategy_preview) and the
 * first refusal is shown on its field. PAPER ONLY: there is no live control.
 */
export function StrategyForm({ kind, initial, bot, preset, onSubmit, onCancel, onPreview, onDirty, mode = "bot", onState, onActions }: StrategyFormProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { strategy, catalog } = useDeskContext();
  const [cfg, setCfg] = useState<StrategyConfig>(initial);
  const [invalid, setInvalid] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<PreviewDto | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  /** Rust's verdict on the current config: undefined while unknown. */
  const [verdict, setVerdict] = useState<StrategyErrorDto | null | undefined>(undefined);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [symbols, setSymbols] = useState<string[] | null>(null);
  const [symbolsError, setSymbolsError] = useState(false);
  const seq = useRef(0);
  const edit = !!bot;
  const cycleLocked = edit && bot.openCycle !== null;
  const slotLocked = edit && !cycleLocked && bot.acceptingNewCycles;
  const spot = cfg.market === "spot";
  const futuresOk = catalog.exchanges?.find((e) => e.id === cfg.exchangeId)?.supportsFutures ?? true;
  const maxLeverage = strategy.risk?.maxLeverage ?? null;
  const breakerPct = strategy.risk?.portfolioDdPct ?? null;
  const dirty = JSON.stringify(cfg) !== JSON.stringify(initial);
  const backtest = mode === "backtest";

  useEffect(() => {
    onDirty?.(dirty);
  }, [dirty, onDirty]);

  // Leaving with unsaved edits asks first. A save in flight (it navigates on
  // success) and a confirmed discard pass through.
  const passGuard = useRef(false);
  const [held, setHeld] = useState<(() => void) | null>(null);
  const guard = useCallback((proceed: () => void) => {
    if (passGuard.current) return true;
    setHeld(() => proceed);
    return false;
  }, []);
  useNavigationGuard(dirty && !backtest, guard);

  const markInvalid = useCallback((key: string, bad: boolean) => {
    setInvalid((prev) => {
      if (prev.has(key) === bad) return prev;
      const next = new Set(prev);
      if (bad) next.add(key);
      else next.delete(key);
      return next;
    });
  }, []);
  const inv = (key: string) => (bad: boolean) => markInvalid(key, bad);

  // The pair list for the chosen market (Binance public list via Rust).
  useEffect(() => {
    let alive = true;
    setSymbols(null);
    setSymbolsError(false);
    exchangeSymbols(cfg.exchangeId, cfg.market === "futures")
      .then((s) => alive && (setSymbols(s), setSymbolsError(s.length === 0)))
      .catch(() => alive && setSymbolsError(true));
    return () => {
      alive = false;
    };
  }, [cfg.exchangeId, cfg.market]);

  // Rust preview on every settled change.
  useEffect(() => {
    if (invalid.size > 0) return;
    const id = ++seq.current;
    setVerdict(undefined);
    const timer = window.setTimeout(() => {
      strategyPreview(cfg)
        .then((p) => {
          if (id !== seq.current) return;
          setPreview(p);
          setPreviewError(null);
          setVerdict(p.error);
        })
        .catch((e: unknown) => {
          if (id !== seq.current) return;
          setPreview(null);
          setPreviewError(errorMessage(e, "priceUnavailable"));
          // No price for the preview: Rust still judges the bounds.
          strategyValidate(cfg)
            .then((r) => id === seq.current && setVerdict(r.error))
            .catch(() => id === seq.current && setVerdict(undefined));
        });
    }, 250);
    return () => window.clearTimeout(timer);
  }, [cfg, invalid.size]);


  const set = (patch: Partial<StrategyConfig>) => setCfg((c) => ({ ...c, ...patch }));
  const setDca = (patch: Partial<DcaParams>) =>
    setCfg((c) => (c.params.kind === "dca" ? { ...c, params: { ...c.params, ...patch } } : c));
  const setGrid = (patch: Partial<GridParams>) =>
    setCfg((c) => (c.params.kind === "grid" ? { ...c, params: { ...c.params, ...patch } } : c));
  const setRestart = (patch: Partial<StrategyConfig["restart"]>) => setCfg((c) => ({ ...c, restart: { ...c.restart, ...patch } }));

  // The first Rust refusal, on its field (the summary repeats it).
  const rustError = verdict ?? null;
  const err = (field: string) => (rustError && rustError.field === field ? strategyErrorText(t, rustError.code) : null);
  const invalidText = t("strategy.form.notANumber");
  const usdt = (v: number) => `${formatNumber(v, locale, { maximumFractionDigits: 2 })} USDT`;

  // Spot is long-only at 1x (Rust refuses anything else).
  const setMarket = (market: StrategyConfig["market"]) =>
    setCfg((c) => ({ ...c, market, ...(market === "spot" ? { side: "long" as StrategySide, leverage: 1 } : {}) }));

  const sideOptions: StrategySide[] = kind === "dca" ? ["long", "short"] : ["long", "short", "neutral"];
  const pairLocked = cycleLocked || slotLocked;
  const paramLocked = cycleLocked;

  const symbolKnown = symbols === null || symbols.length === 0 || symbols.includes(cfg.symbol);
  const presetParity = preset ? matchesPreset(cfg, preset) : null;
  const available =
    strategy.risk !== null
      ? (strategy.risk.balance * strategy.risk.budgetCapPct) / 100 -
        strategy.risk.reservedBudget +
        (bot && (bot.acceptingNewCycles || bot.openCycle) ? bot.budget : 0)
      : null;

  useEffect(() => {
    onPreview({ preview, error: previewError, cfg, available });
  }, [preview, previewError, cfg, available, onPreview]);

  const accepted = invalid.size === 0 && verdict === null;
  useEffect(() => {
    onState?.({ cfg: { ...cfg, name: cfg.name.trim() }, ok: accepted });
  }, [cfg, accepted, onState]);

  // A leveraged DCA without a stop is confirmed first (LeverageRiskDialog).
  const [riskHeld, setRiskHeld] = useState<boolean | null>(null);
  const submit = async (startAfter: boolean, confirmed = false) => {
    if (!confirmed && needsLeverageConfirm(cfg)) {
      setRiskHeld(startAfter);
      return;
    }
    setSaving(true);
    passGuard.current = true;
    setSubmitError(null);
    const out: StrategyConfig = {
      ...cfg,
      name: cfg.name.trim(),
      // Provenance only while the config is still the preset's.
      presetId: preset ? (matchesPreset(cfg, preset) ? preset.id : null) : cfg.presetId,
    };
    const e = await onSubmit(out, startAfter);
    passGuard.current = false;
    setSaving(false);
    if (e) setSubmitError(e);
  };
  const submitField = submitError ? parseStrategyError(submitError).field : null;
  const fieldError = (field: string) => err(field) ?? (submitField === field && submitError ? strategyErrorText(t, submitError) : null);

  const canSubmit = invalid.size === 0 && !saving && verdict === null && (!edit || dirty);
  const p = cfg.params;

  const submitRef = useRef(submit);
  submitRef.current = submit;
  const canSubmitRef = useRef(canSubmit);
  canSubmitRef.current = canSubmit;
  const stableSubmit = useCallback((startAfter: boolean) => {
    if (canSubmitRef.current) void submitRef.current(startAfter);
  }, []);
  useEffect(() => {
    onActions?.({ canSubmit, saving, submit: stableSubmit });
  }, [canSubmit, saving, stableSubmit, onActions]);

  return (
    <>
      <form
        className="ae-sform"
        onSubmit={(e) => {
          e.preventDefault();
          if (canSubmit && !backtest) void submit(false);
        }}
      >
        <p className="ae-sform__guide">
          <Link to={`/guide?s=${kind === "grid" ? "grid-settings" : "dca-settings"}`} className="ae-link">
            {t("guide.formLink")}
          </Link>
        </p>
        {preset ? (
          <p className="ae-banner" data-tone={presetParity ? "info" : "warn"}>
            {presetParity
              ? t("strategy.form.fromPreset", { id: preset.id })
              : t("strategy.form.presetDiffers", { id: preset.id })}
          </p>
        ) : null}
        {preset && preset.verdict === "failed" ? (
          <p className="ae-banner" data-tone="danger">
            {t("strategy.form.presetFailed", {
              reasons: preset.failReasons.map((r) => t(`strategy.preset.reason.${r}`, { defaultValue: r })).join(" · "),
            })}
          </p>
        ) : null}
        {cycleLocked ? (
          <p className="ae-banner" data-tone="warn">
            {t("strategy.form.cycleLocked")}
          </p>
        ) : slotLocked ? (
          <p className="ae-banner" data-tone="info">
            {t("strategy.form.slotLocked")}
          </p>
        ) : null}

        <Panel title={t("strategy.form.section.general")}>
          <div className="ae-sform__grid">
            <FieldShell label={t("strategy.field.name")} htmlFor="sf-name" error={fieldError("name")}>
              <input
                id="sf-name"
                className="ae-field__input"
                value={cfg.name}
                maxLength={40}
                onChange={(e) => set({ name: e.target.value })}
              />
            </FieldShell>
            {/* A backtest's candles come from DataHub, not this live price source. */}
            {!backtest ? (
              <ReadOnlyField
                label={t("strategy.field.priceSource")}
                value="Binance"
                hint={fieldError("exchangeId") ? <span className="ae-sfield__error">{fieldError("exchangeId")}</span> : t("strategy.hint.priceSource")}
              />
            ) : null}
            <Segmented
              label={t("table.market")}
              value={cfg.market}
              disabled={pairLocked}
              options={[
                { value: "spot", label: t("filters.market.spot") },
                { value: "futures", label: t("filters.market.futures"), disabled: !futuresOk, reason: t("strategy.hint.noFutures") },
              ]}
              onChange={setMarket}
            />
            {kind === "grid" && spot ? (
              <ReadOnlyField label={t("strategy.field.side")} value={t("strategy.side.long")} hint={t("strategy.hint.gridSpotInventory")} />
            ) : (
              <Segmented
                label={t(kind === "grid" ? "strategy.field.gridMode" : "strategy.field.side")}
                value={cfg.side}
                disabled={pairLocked}
                error={fieldError("side")}
                options={sideOptions.map((s) => ({
                  value: s,
                  label: t(`strategy.side.${s}`),
                  disabled: spot && s !== "long",
                  reason: t("strategy.hint.spotLongOnly"),
                }))}
                onChange={(side) =>
                  setCfg((c) => ({
                    ...c,
                    side,
                    params: c.params.kind === "grid" && side !== "long" ? { ...c.params, trailingUp: false, trailUpLimit: null } : c.params,
                  }))
                }
              />
            )}
            <FieldShell
              label={t("strategy.field.pair")}
              htmlFor="sf-pair"
              error={fieldError("symbol") ?? (!symbolKnown ? t("strategy.errors.symbolUnknown") : null)}
              hint={
                symbolsError
                  ? t("strategy.errors.symbolListUnavailable")
                  : preset
                    ? t(`strategy.preset.universe.${preset.universe.rule}`, { n: preset.universe.topN, days: preset.universe.volumeWindowDays })
                    : t("strategy.hint.pairUsdt")
              }
            >
              <input
                id="sf-pair"
                className="ae-field__input mono"
                list="sf-pairs"
                value={cfg.symbol}
                disabled={pairLocked}
                autoComplete="off"
                spellCheck={false}
                placeholder="BTCUSDT"
                onChange={(e) => set({ symbol: e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "") })}
              />
              <datalist id="sf-pairs">
                {(symbols ?? []).map((s) => (
                  <option key={s} value={s} />
                ))}
              </datalist>
            </FieldShell>
            <NumField
              label={t("strategy.field.budget")}
              unit="USDT"
              value={cfg.budget}
              disabled={paramLocked}
              onChange={(budget) => set({ budget })}
              onInvalid={inv("budget")}
              invalidText={invalidText}
              error={fieldError("budget")}
              hint={available !== null && !backtest ? t("strategy.hint.budgetAvailable", { value: usdt(Math.max(0, available)) }) : undefined}
            />
            {!spot ? (
              <>
                <NumField
                  label={t("table.leverage")}
                  unit="x"
                  integer
                  wireMax={U8}
                  value={cfg.leverage}
                  disabled={paramLocked}
                  onChange={(leverage) => set({ leverage })}
                  onInvalid={inv("leverage")}
                  invalidText={invalidText}
                  error={fieldError("leverage")}
                  hint={maxLeverage !== null ? t("strategy.hint.leverageMax", { max: maxLeverage }) : undefined}
                />
                <ReadOnlyField
                  label={t("strategy.field.marginMode")}
                  value={t("strategy.field.isolated")}
                  hint={t("strategy.hint.marginMode")}
                />
              </>
            ) : null}
          </div>
        </Panel>

        {p.kind === "dca" ? (
          <Panel title={t("strategy.form.section.dca")}>
            <div className="ae-sform__grid">
              <Segmented
                label={t("strategy.field.sizing")}
                value={p.baseOrder !== null || p.safetyOrder !== null ? "fixed" : "weights"}
                disabled={paramLocked}
                options={[
                  { value: "weights", label: t("strategy.sizing.weights") },
                  { value: "fixed", label: t("strategy.sizing.fixed") },
                ]}
                onChange={(v) =>
                  setDca(
                    v === "fixed"
                      ? { baseOrder: Math.max(5, Math.round(cfg.budget / 10)), safetyOrder: Math.max(5, Math.round(cfg.budget / 10)), baseWeight: null, safetyWeight: null }
                      : { baseOrder: null, safetyOrder: null, baseWeight: 1, safetyWeight: 1 },
                  )
                }
                hint={t(p.baseOrder !== null || p.safetyOrder !== null ? "strategy.hint.sizingFixed" : "strategy.hint.sizingWeights")}
              />
              {p.baseOrder !== null || p.safetyOrder !== null ? (
                <>
                  <NumField label={t("strategy.field.baseOrder")} unit="USDT" value={p.baseOrder ?? 0} disabled={paramLocked} onChange={(baseOrder) => setDca({ baseOrder })} onInvalid={inv("baseOrder")} invalidText={invalidText} error={fieldError("baseOrder")} />
                  <NumField label={t("strategy.field.safetyOrder")} unit="USDT" value={p.safetyOrder ?? 0} disabled={paramLocked || p.maxSo === 0} onChange={(safetyOrder) => setDca({ safetyOrder })} onInvalid={inv("safetyOrder")} invalidText={invalidText} error={fieldError("safetyOrder")} />
                </>
              ) : (
                <>
                  <NumField label={t("strategy.field.baseWeight")} value={p.baseWeight ?? 1} disabled={paramLocked} onChange={(baseWeight) => setDca({ baseWeight })} onInvalid={inv("baseWeight")} invalidText={invalidText} error={fieldError("baseWeight")} />
                  <NumField label={t("strategy.field.safetyWeight")} value={p.safetyWeight ?? 1} disabled={paramLocked || p.maxSo === 0} onChange={(safetyWeight) => setDca({ safetyWeight })} onInvalid={inv("safetyWeight")} invalidText={invalidText} error={fieldError("safetyWeight")} />
                </>
              )}
              <NumField label={t("strategy.field.maxSo")} integer wireMax={U8} value={p.maxSo} disabled={paramLocked} onChange={(maxSo) => setDca({ maxSo })} onInvalid={inv("maxSo")} invalidText={invalidText} error={fieldError("maxSo")} hint={t("strategy.hint.maxSo")} />
              {p.maxSo > 0 ? (
                <>
                  <NumField label={t("strategy.field.soStep")} unit="%" value={p.soStepPct} disabled={paramLocked} onChange={(soStepPct) => setDca({ soStepPct })} onInvalid={inv("soStepPct")} invalidText={invalidText} error={fieldError("soStepPct")} hint={t("strategy.hint.soStep")} />
                  <NumField label={t("strategy.field.stepScale")} value={p.stepScale} disabled={paramLocked} onChange={(stepScale) => setDca({ stepScale })} onInvalid={inv("stepScale")} invalidText={invalidText} error={fieldError("stepScale")} hint={t("strategy.hint.stepScale")} />
                  <NumField
                    label={t("strategy.field.volumeScale")}
                    value={p.volumeScale}
                    disabled={paramLocked}
                    onChange={(volumeScale) => setDca({ volumeScale })}
                    onInvalid={inv("volumeScale")}
                    invalidText={invalidText}
                    error={fieldError("volumeScale")}
                    hint={p.volumeScale > 1 ? t("strategy.hint.volumeScaleGrows") : t("strategy.hint.volumeScale")}
                  />
                </>
              ) : null}
              <NumField label={t("strategy.field.tp")} unit="%" value={p.tpPct} disabled={paramLocked} onChange={(tpPct) => setDca({ tpPct })} onInvalid={inv("tpPct")} invalidText={invalidText} error={fieldError("tpPct")} hint={t("strategy.hint.tp")} />
            </div>
            <div className="ae-sform__options">
              <CheckRow label={t("strategy.field.trailing")} checked={p.trailingPct !== null} disabled={paramLocked} onChange={(on) => setDca({ trailingPct: on ? 0.5 : null })} />
              {p.trailingPct !== null ? (
                <NumField label={t("strategy.field.trailingPct")} unit="%" value={p.trailingPct} disabled={paramLocked} onChange={(trailingPct) => setDca({ trailingPct })} onInvalid={inv("trailingPct")} invalidText={invalidText} error={fieldError("trailingPct")} />
              ) : null}
              <CheckRow
                label={t("strategy.field.sl")}
                checked={p.slPct !== null}
                disabled={paramLocked}
                onChange={(on) => setDca({ slPct: on ? Math.min(90, Math.round((preview?.dca?.maxCoveragePct ?? 10) + 5)) : null })}
                hint={p.slPct === null ? <span className="ae-warntext">{t("strategy.warnings.noStopLoss")}</span> : undefined}
              />
              {p.slPct !== null ? (
                <NumField label={t("strategy.field.slPct")} unit="%" value={p.slPct} disabled={paramLocked} onChange={(slPct) => setDca({ slPct })} onInvalid={inv("slPct")} invalidText={invalidText} error={fieldError("slPct")} hint={t("strategy.hint.sl")} />
              ) : null}
              <CheckRow label={t("strategy.field.maxDuration")} checked={p.maxDurationMin !== null} disabled={paramLocked} onChange={(on) => setDca({ maxDurationMin: on ? 168 * 60 : null })} />
              {p.maxDurationMin !== null ? (
                <NumField label={t("strategy.field.maxDurationHours")} unit="h" value={toHours(p.maxDurationMin)} disabled={paramLocked} onChange={(h) => setDca({ maxDurationMin: toMin(h) })} onInvalid={inv("maxDurationMin")} invalidText={invalidText} error={fieldError("maxDurationMin")} />
              ) : null}
            </div>
          </Panel>
        ) : (
          <Panel title={t("strategy.form.section.grid")}>
            <div className="ae-sform__grid">
              <Segmented
                label={t("strategy.field.rangeMode")}
                value={p.range.type}
                disabled={paramLocked}
                error={fieldError("range")}
                options={[
                  { value: "relative", label: t("strategy.range.relative") },
                  { value: "absolute", label: t("strategy.range.absolute") },
                ]}
                onChange={(v) => {
                  const ref = preview?.referencePrice ?? null;
                  if (v === "relative") setGrid({ range: { type: "relative", lowerPct: 8, upperPct: 8 } });
                  else if (preview?.grid) setGrid({ range: { type: "absolute", lower: round(preview.grid.lower), upper: round(preview.grid.upper) } });
                  else if (ref) setGrid({ range: { type: "absolute", lower: round(ref * 0.92), upper: round(ref * 1.08) } });
                }}
                hint={p.range.type === "relative" ? t("strategy.hint.rangeRelative") : t("strategy.hint.rangeAbsolute")}
              />
              {p.range.type === "relative" ? (
                <>
                  <NumField label={t("strategy.field.lowerPct")} unit="%" value={p.range.lowerPct} disabled={paramLocked} onChange={(lowerPct) => p.range.type === "relative" && setGrid({ range: { ...p.range, lowerPct } })} onInvalid={inv("lowerPct")} invalidText={invalidText} />
                  <NumField label={t("strategy.field.upperPct")} unit="%" value={p.range.upperPct} disabled={paramLocked} onChange={(upperPct) => p.range.type === "relative" && setGrid({ range: { ...p.range, upperPct } })} onInvalid={inv("upperPct")} invalidText={invalidText} />
                </>
              ) : (
                <>
                  <NumField label={t("strategy.field.lower")} value={p.range.lower} disabled={paramLocked} onChange={(lower) => p.range.type === "absolute" && setGrid({ range: { ...p.range, lower } })} onInvalid={inv("lower")} invalidText={invalidText} />
                  <NumField label={t("strategy.field.upper")} value={p.range.upper} disabled={paramLocked} onChange={(upper) => p.range.type === "absolute" && setGrid({ range: { ...p.range, upper } })} onInvalid={inv("upper")} invalidText={invalidText} />
                </>
              )}
              <NumField label={t("strategy.field.nGrids")} integer wireMax={U16} value={p.nGrids} disabled={paramLocked} onChange={(nGrids) => setGrid({ nGrids })} onInvalid={inv("nGrids")} invalidText={invalidText} error={fieldError("nGrids")} hint={t("strategy.hint.nGrids", { levels: p.nGrids + 1 })} />
              <Segmented
                label={t("strategy.field.spacing")}
                value={p.spacing}
                disabled={paramLocked}
                options={[
                  { value: "geom", label: t("strategy.spacing.geom") },
                  { value: "arith", label: t("strategy.spacing.arith") },
                ]}
                onChange={(spacing) => setGrid({ spacing })}
              />
            </div>
            <div className="ae-sform__options">
              <CheckRow
                label={t("strategy.field.stopOut")}
                checked={p.stopOutPct !== null}
                disabled={paramLocked}
                onChange={(on) => setGrid({ stopOutPct: on ? 3 : null })}
                hint={p.stopOutPct === null ? <span className="ae-warntext">{t("strategy.warnings.noStopOut")}</span> : undefined}
              />
              {p.stopOutPct !== null ? (
                <NumField label={t("strategy.field.stopOutPct")} unit="%" value={p.stopOutPct} disabled={paramLocked} onChange={(stopOutPct) => setGrid({ stopOutPct })} onInvalid={inv("stopOutPct")} invalidText={invalidText} error={fieldError("stopOutPct")} hint={t("strategy.hint.stopOut")} />
              ) : null}
              {cfg.side === "long" ? (
                <>
                  <CheckRow label={t("strategy.field.trailingUp")} checked={p.trailingUp} disabled={paramLocked} error={fieldError("trailingUp")} onChange={(trailingUp) => setGrid({ trailingUp, trailUpLimit: trailingUp ? p.trailUpLimit : null })} hint={t("strategy.hint.trailingUp")} />
                  {p.trailingUp ? (
                    <>
                      <CheckRow label={t("strategy.field.trailUpLimitOn")} checked={p.trailUpLimit !== null} disabled={paramLocked} onChange={(on) => setGrid({ trailUpLimit: on ? round((preview?.grid?.upper ?? 0) * 1.2) || null : null })} />
                      {p.trailUpLimit !== null ? (
                        <NumField label={t("strategy.field.trailUpLimit")} value={p.trailUpLimit} disabled={paramLocked} onChange={(trailUpLimit) => setGrid({ trailUpLimit })} onInvalid={inv("trailUpLimit")} invalidText={invalidText} error={fieldError("trailUpLimit")} />
                      ) : null}
                    </>
                  ) : null}
                </>
              ) : null}
              <CheckRow label={t("strategy.field.gridTp")} checked={p.takeProfitPct !== null} disabled={paramLocked} onChange={(on) => setGrid({ takeProfitPct: on ? 10 : null })} />
              {p.takeProfitPct !== null ? (
                <NumField label={t("strategy.field.gridTpPct")} unit="%" value={p.takeProfitPct} disabled={paramLocked} onChange={(takeProfitPct) => setGrid({ takeProfitPct })} onInvalid={inv("takeProfitPct")} invalidText={invalidText} error={fieldError("takeProfitPct")} hint={t("strategy.hint.gridTp")} />
              ) : null}
              <CheckRow label={t("strategy.field.maxDuration")} checked={p.maxDurationMin !== null} disabled={paramLocked} onChange={(on) => setGrid({ maxDurationMin: on ? 168 * 60 : null })} />
              {p.maxDurationMin !== null ? (
                <NumField label={t("strategy.field.maxDurationHours")} unit="h" value={toHours(p.maxDurationMin)} disabled={paramLocked} onChange={(h) => setGrid({ maxDurationMin: toMin(h) })} onInvalid={inv("maxDurationMin")} invalidText={invalidText} error={fieldError("maxDurationMin")} />
              ) : null}
            </div>
          </Panel>
        )}

        {!backtest ? <StrategyLevelsTable preview={preview} /> : null}

        <Panel title={t("strategy.form.section.start")}>
          <div className="ae-sform__grid">
            <Segmented
              label={t("strategy.field.start")}
              value={cfg.start.type}
              error={fieldError("start")}
              options={[
                { value: "immediately", label: t("strategy.start.immediately") },
                { value: "priceCross", label: t("strategy.start.priceCross") },
              ]}
              onChange={(v) =>
                set({
                  start:
                    v === "immediately"
                      ? { type: "immediately" }
                      : { type: "priceCross", price: round(preview?.referencePrice ?? 0) || 1, direction: "down" },
                })
              }
            />
            {cfg.start.type === "priceCross" ? (
              <>
                <NumField
                  label={t("strategy.field.startPrice")}
                  value={cfg.start.price}
                  onChange={(price) => cfg.start.type === "priceCross" && set({ start: { ...cfg.start, price } })}
                  onInvalid={inv("startPrice")}
                  invalidText={invalidText}
                  hint={t("strategy.hint.startPrice")}
                />
                <Segmented
                  label={t("strategy.field.crossDirection")}
                  value={cfg.start.direction}
                  options={[
                    { value: "down", label: t("strategy.cross.down") },
                    { value: "up", label: t("strategy.cross.up") },
                  ]}
                  onChange={(direction) => cfg.start.type === "priceCross" && set({ start: { ...cfg.start, direction } })}
                />
              </>
            ) : null}
            <NumField
              label={t("strategy.field.cooldown")}
              unit="min"
              integer
              wireMax={U32}
              value={cfg.restart.cooldownMin}
              onChange={(cooldownMin) => setRestart({ cooldownMin })}
              onInvalid={inv("cooldown")}
              invalidText={invalidText}
              error={fieldError("restart")}
            />
          </div>
          <div className="ae-sform__options">
            <CheckRow label={t("strategy.field.maxCycles")} checked={cfg.restart.maxCycles !== null} onChange={(on) => setRestart({ maxCycles: on ? 10 : null })} />
            {cfg.restart.maxCycles !== null ? (
              <NumField label={t("strategy.field.maxCyclesN")} integer wireMax={U32} value={cfg.restart.maxCycles} onChange={(maxCycles) => setRestart({ maxCycles })} onInvalid={inv("maxCycles")} invalidText={invalidText} error={fieldError("maxCycles")} />
            ) : null}
            <CheckRow label={t("strategy.field.afterStop")} checked={cfg.restart.afterStop} onChange={(afterStop) => setRestart({ afterStop })} hint={t("strategy.hint.afterStop")} />
            {!spot ? (
              <CheckRow label={t("strategy.field.afterLiquidation")} checked={cfg.restart.afterLiquidation} onChange={(afterLiquidation) => setRestart({ afterLiquidation })} hint={t("strategy.hint.afterLiquidation")} />
            ) : null}
            <CheckRow
              label={t("strategy.field.priceBand")}
              checked={cfg.restart.priceBand !== null}
              onChange={(on) => {
                const ref = preview?.referencePrice ?? 0;
                setRestart({ priceBand: on && ref > 0 ? [round(ref * 0.8), round(ref * 1.2)] : null });
              }}
              hint={t("strategy.hint.priceBand")}
            />
            {cfg.restart.priceBand !== null ? (
              <div className="ae-sform__pair">
                <NumField label={t("strategy.field.bandMin")} value={cfg.restart.priceBand[0]} onChange={(v) => cfg.restart.priceBand && setRestart({ priceBand: [v, cfg.restart.priceBand[1]] })} onInvalid={inv("bandMin")} invalidText={invalidText} error={fieldError("priceBand")} />
                <NumField label={t("strategy.field.bandMax")} value={cfg.restart.priceBand[1]} onChange={(v) => cfg.restart.priceBand && setRestart({ priceBand: [cfg.restart.priceBand[0], v] })} onInvalid={inv("bandMax")} invalidText={invalidText} />
              </div>
            ) : null}
            <CheckRow label={t("strategy.field.endAt")} checked={cfg.restart.endAtMs !== null} onChange={(on) => setRestart({ endAtMs: on ? Date.now() + 7 * 86_400_000 : null })} hint={t("strategy.hint.endAt")} />
            {cfg.restart.endAtMs !== null ? (
              <FieldShell label={t("strategy.field.endAtTime")} htmlFor="sf-end">
                <input
                  id="sf-end"
                  type="datetime-local"
                  className="ae-field__input mono"
                  value={localInput(cfg.restart.endAtMs)}
                  onChange={(e) => {
                    const ms = new Date(e.target.value).getTime();
                    if (Number.isFinite(ms) && ms > 0) setRestart({ endAtMs: ms });
                  }}
                />
              </FieldShell>
            ) : null}
          </div>
        </Panel>

        <Panel title={t("strategy.form.section.protection")}>
          <div className="ae-sform__options">
            <CheckRow
              label={t("strategy.field.ddStop")}
              checked={cfg.maxDrawdownPct !== null}
              onChange={(on) => set({ maxDrawdownPct: on ? 25 : null })}
              hint={cfg.maxDrawdownPct === null ? <span className="ae-warntext">{t("strategy.warnings.noDrawdownStop")}</span> : undefined}
            />
            {cfg.maxDrawdownPct !== null ? (
              <NumField label={t("strategy.field.ddStopPct")} unit="%" value={cfg.maxDrawdownPct} onChange={(maxDrawdownPct) => set({ maxDrawdownPct })} onInvalid={inv("maxDrawdownPct")} invalidText={invalidText} error={fieldError("maxDrawdownPct")} hint={t("strategy.hint.ddStop")} />
            ) : null}
            {!backtest ? (
              <CheckRow label={t("strategy.field.btcGate")} checked={cfg.pauseOnBtcBreak} onChange={(pauseOnBtcBreak) => set({ pauseOnBtcBreak })} hint={t("strategy.hint.btcGate")} />
            ) : null}
            {!backtest ? (
              <CheckRow
                label={
                  breakerPct !== null
                    ? t("strategy.field.portfolioBreaker", { pct: formatNumber(breakerPct, locale, { maximumFractionDigits: 1 }) })
                    : t("strategy.risk.breaker")
                }
                checked={cfg.portfolioBreaker}
                onChange={(portfolioBreaker) => set({ portfolioBreaker })}
                hint={cfg.portfolioBreaker ? undefined : t("strategy.hint.portfolioBreakerOff")}
              />
            ) : null}
          </div>
        </Panel>

        {submitError && !backtest ? (
          <p className="ae-banner" data-tone="danger" role="alert">
            {strategyErrorText(t, submitError)}
          </p>
        ) : null}
        {!backtest && !onActions ? (
          <div className="ae-sform__actions">
            <Button variant="secondary" size="sm" onClick={onCancel}>
              {t("states.cancel")}
            </Button>
            <span className="ae-toolbar__spacer" />
            {!edit ? (
              <Button variant="secondary" size="sm" disabled={!canSubmit} onClick={() => void submit(true)}>
                {t("strategy.form.createAndStart")}
              </Button>
            ) : null}
            <Button type="submit" size="sm" disabled={!canSubmit}>
              {t(edit ? "strategy.form.save" : "strategy.form.create")}
            </Button>
          </div>
        ) : null}
      </form>
      <LeverageRiskDialog
        open={riskHeld !== null}
        cfg={cfg}
        preview={preview}
        onConfirm={() => {
          const startAfter = riskHeld ?? false;
          setRiskHeld(null);
          void submit(startAfter, true);
        }}
        onCancel={() => setRiskHeld(null)}
      />
      {/* Outside the form: a submit inside the dialog must not bubble into the bot form. */}
      <ConfirmDialog
        open={held !== null}
        title={t("strategy.form.leaveTitle")}
        confirmLabel={t("strategy.form.discard")}
        onConfirm={() => {
          const go = held;
          setHeld(null);
          passGuard.current = true;
          go?.();
          passGuard.current = false;
        }}
        onCancel={() => setHeld(null)}
      />
    </>
  );
}

/** Six significant digits: enough for any USDT pair's tick, no float noise. */
function round(v: number): number {
  return Number.isFinite(v) && v > 0 ? Number(v.toPrecision(6)) : 0;
}
