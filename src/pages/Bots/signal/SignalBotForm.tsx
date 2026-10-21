import { Link } from "@/app/router/router";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { ComboPicker } from "@/components/BotDesk/ComboPicker/ComboPicker";
import {
  draftFromTarget,
  draftValid,
  targetFromDraft,
  TakeProfitPicker,
  type OverrideDraft,
  type TargetDraft,
} from "@/components/BotDesk/TakeProfitPicker/TakeProfitPicker";
import { Button } from "@/components/ui/Button/Button";
import { Chip, LiveChip, PaperChip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { CLOSE_WORD } from "@/components/Account/AccountPanel/AccountPanel";
import { ModePicker } from "@/components/LiveMode/LiveMode";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { Panel } from "@/components/ui/Panel/Panel";
import { ReadOnlyField } from "@/components/ui/ReadOnlyField/ReadOnlyField";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { localeForLanguage } from "@/i18n";
import { capitalAboveCap, EXCHANGE_MIN_ORDER_USDT, MAX_BOT_POSITIONS } from "@/lib/botLimits";
import { formatDecimalInput, parseDecimal } from "@/lib/decimal";
import { formatNumber, formatPercent, formatSignedPercent, formatSignedUsdt, formatUsdt } from "@/lib/format";
import { botDefaultConfig, botMode, PILOT_NOTIONAL_USDT, type BotConfig, type BotKind, type OpenPosition, type SizingMode, type TradeMode } from "@/lib/ipc/bot/bot";
import { liveWordKey, signalStartAction, type Blocker } from "@/lib/liveMode";
import { errorMessage } from "@/lib/ipc/bridge";
import type { ExchangeInfo } from "@/lib/ipc/exchange/exchange";
import {
  DEFAULT_RISK_PCT,
  estimateRiskSize,
  EXAMPLE_STOP_FRACS,
  FUTURES_FEE_RATE,
  MAX_RISK_PCT,
  maxLossEffect,
  MIN_RISK_PCT,
  SPOT_FEE_RATE,
  type MaxLossEffect,
  type SizeEstimate,
} from "@/lib/sizing";
import { FieldShell } from "@/pages/Bots/strategy/formControls";
import "@/pages/Bots/strategy/StrategyForm.css";
import "@/pages/Bots/bots.css";
import "./SignalBotForm.css";
import { TrendMismatchNote } from "@/components/Desk/MarketTrend";

/** Paper / Live wiring for one signal bot (every bot shows its mode). */
export interface BotLiveControl {
  /** Real entries left in the pilot (0 = full size). */
  pilotLeft: number;
  onEndPilot: () => Promise<string | null>;
  /** Why Live cannot be chosen now (empty = it can). */
  blockers: Blocker[];
  /** Saves `config`, shows the money at risk and the typed LIVE, then starts. */
  onStartLive: (config: BotConfig) => void;
  /** Back to Paper: disarms and stops; Paper once no real position is held ("Switch later"). */
  onToPaper: () => Promise<string | null>;
  /** This bot's open real positions (the Paper switch lists them). */
  realPositions: OpenPosition[];
  /** "Close positions and switch": reduce-only closes of `realPositions`, then Paper. */
  onCloseAndPaper: () => Promise<string | null>;
  /** Orders go to the Binance testnet: say "testnet", never "real money". */
  testnet: boolean;
  /** The last real-money refusal, shown in the Mode panel. */
  error?: string | null;
}

interface SignalBotFormProps {
  kind: BotKind;
  config: BotConfig | null;
  running: boolean;
  busy: boolean;
  /** Start is refused (the daily stop). A running bot stays stoppable. */
  disabled: boolean;
  /**
   * The exchange cannot change: the bot is switched to real money or holds
   * real positions (Rust refuses it too: liveExchangeLocked / botHasLivePositions).
   */
  exchangeLocked?: boolean;
  /** i18n key of why Start is refused, shown on the button's tooltip. */
  disabledReasonKey?: string | null;
  maxLeverage: number;
  /**
   * The risk level's cap per position in USDT (balance × max capital %), or
   * null while the risk state loads. Capital above it is refused by Rust.
   */
  maxCapitalQuote: number | null;
  /** The risk level's position limit per signal bot (null while loading). */
  levelMaxPositions?: number | null;
  /** Exchanges the bot may price against (paper needs no key). */
  exchanges: ExchangeInfo[];
  /** Paper / Live for this bot. */
  live?: BotLiveControl;
  /** Configure + start this bot with the built config (single awaited chain). */
  onStart: (config: BotConfig) => void;
  onStop: () => void;
}

/** Where the form's values came from; Start stays off until it has some. */

type Seed = "none" | "defaults" | "config";

/**
 * One signal bot's settings in the DCA / Grid form language:
 * panels with a field grid. Every value maps onto Rust's BotConfig; Start
 * runs bot_configure then bot_start in one awaited chain. No number is
 * invented: a saved bot shows its config, a new one the server's defaults
 * (bot_default_config), and until one has arrived the fields are empty and
 * Start is off.
 */
export function SignalBotForm({
  kind,
  config,
  running,
  busy,
  disabled,
  exchangeLocked = false,
  disabledReasonKey,
  maxLeverage,
  maxCapitalQuote,
  levelMaxPositions = null,
  exchanges,
  live,
  onStart,
  onStop,
}: SignalBotFormProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const ids = useId();
  // Futures + Pump trade the leveraged futures market; only Spot is 1x long-only.
  const futuresMarket = kind !== "spot";
  const usable = futuresMarket ? exchanges.filter((e) => e.supportsFutures) : exchanges;
  // The exchange list can arrive after the first render: fall back to the
  // first usable exchange instead of freezing an empty selection.
  const [exchangeSel, setExchangeSel] = useState(config?.exchangeId ?? "");
  // Paper / Live as chosen on this screen: the saved mode, Paper for a new bot.
  const [chosen, setChosen] = useState<TradeMode>(botMode(config));
  const exchangeId = exchangeSel || usable[0]?.id || "";
  const [capital, setCapital] = useState(config ? formatDecimalInput(config.capital) : "");
  const [positions, setPositions] = useState(config ? formatDecimalInput(config.maxPositions) : "");
  const [leverage, setLeverage] = useState(config ? formatDecimalInput(config.leverage) : "");
  // A saved config without `sizing` predates it and the server runs it as "fixed".
  const [sizing, setSizing] = useState<SizingMode>(config ? (config.sizing ?? "fixed") : "risk");
  const [riskPct, setRiskPct] = useState(config?.riskPerTradePct != null ? formatDecimalInput(config.riskPerTradePct) : "");
  const [takeProfit, setTakeProfit] = useState<TargetDraft>(draftFromTarget(config?.takeProfit));
  const [tpOverrides, setTpOverrides] = useState<OverrideDraft[]>(overridesFrom(config));
  // Hours on screen, minutes on the wire. Empty = the server's default age.
  const [maxAgeH, setMaxAgeH] = useState(config?.maxSignalAgeMin != null ? formatDecimalInput(config.maxSignalAgeMin / 60) : "");
  const [minConfidence, setMinConfidence] = useState(
    config?.minConfidence != null ? formatDecimalInput(Math.round(config.minConfidence * 100)) : "",
  );
  const [direction, setDirection] = useState(config?.direction ?? "all");
  const [symbols, setSymbols] = useState((config?.symbols ?? []).join(", "));
  const [combos, setCombos] = useState<string[]>(config?.combos ?? []);
  const [engines, setEngines] = useState((config?.engines ?? []).join(", "));
  const [maxLoss, setMaxLoss] = useState(config?.maxLossPct != null ? formatDecimalInput(config.maxLossPct) : "");

  const [seed, setSeed] = useState<Seed>(config ? "config" : "none");
  const seedRef = useRef<Seed>(seed);
  const [defaults, setDefaults] = useState<BotConfig | null>(null);
  const [defaultsError, setDefaultsError] = useState<string | null>(null);
  const [defaultsAttempt, setDefaultsAttempt] = useState(0);

  // The saved config can show up after mount (Start on a new bot saves it).
  // It then replaces the defaults once; later polls never touch a form the
  // user may be editing.
  useEffect(() => {
    if (!config || seedRef.current === "config") return;
    seedRef.current = "config";
    setSeed("config");
    setExchangeSel(config.exchangeId);
    setCapital(formatDecimalInput(config.capital));
    setPositions(formatDecimalInput(config.maxPositions));
    setLeverage(formatDecimalInput(config.leverage));
    setSizing(config.sizing ?? "fixed");
    if (config.riskPerTradePct != null) setRiskPct(formatDecimalInput(config.riskPerTradePct));
    setTakeProfit(draftFromTarget(config.takeProfit));
    setTpOverrides(overridesFrom(config));
    if (config.maxSignalAgeMin != null) setMaxAgeH(formatDecimalInput(config.maxSignalAgeMin / 60));
    setMinConfidence(config.minConfidence != null ? formatDecimalInput(Math.round(config.minConfidence * 100)) : "");
    setDirection(config.direction ?? "all");
    setSymbols((config.symbols ?? []).join(", "));
    setCombos(config.combos ?? []);
    setEngines((config.engines ?? []).join(", "));
    setMaxLoss(config.maxLossPct != null ? formatDecimalInput(config.maxLossPct) : "");
  }, [config]);

  // Server defaults seed a NEW bot and back the fields a saved config leaves
  // empty (risk % on a legacy fixed-size bot, the signal-age limit when its
  // field is cleared). A failure is shown with a retry, never papered over.
  useEffect(() => {
    let alive = true;
    botDefaultConfig(kind, exchangeId || "binance")
      .then((d) => {
        if (!alive) return;
        setDefaults(d);
        setDefaultsError(null);
        setRiskPct((r) => (r === "" && d.riskPerTradePct != null ? formatDecimalInput(d.riskPerTradePct) : r));
        if (seedRef.current !== "none") return;
        seedRef.current = "defaults";
        setSeed("defaults");
        setCapital(formatDecimalInput(d.capital));
        setPositions(formatDecimalInput(d.maxPositions));
        setLeverage(formatDecimalInput(d.leverage));
        setSizing(d.sizing ?? "risk");
        setTakeProfit(draftFromTarget(d.takeProfit));
        if (d.maxSignalAgeMin != null) setMaxAgeH(formatDecimalInput(d.maxSignalAgeMin / 60));
      })
      .catch((e: unknown) => {
        if (alive) setDefaultsError(errorMessage(e, t("bots.defaultsFailed")));
      });
    return () => {
      alive = false;
    };
    // The server's defaults do not depend on the exchange; `defaultsAttempt` is the retry.
  }, [kind, defaultsAttempt]);

  const capitalNum = parseDecimal(capital) || 0;
  const positionsNum = Math.min(MAX_BOT_POSITIONS, Math.max(1, Math.round(parseDecimal(positions)) || 1));
  const leverageNum = futuresMarket ? Math.min(maxLeverage, Math.max(1, Math.round(parseDecimal(leverage)) || 1)) : 1;
  // Sent as typed: an out-of-range value is refused by the server's
  // validate(), never silently corrected here.
  const riskPctNum = parseDecimal(riskPct) || 0;
  // Clamped ONLY for the preview, which must show a real number mid-edit.
  const riskPctForPreview = Math.min(MAX_RISK_PCT, Math.max(MIN_RISK_PCT, riskPctNum || DEFAULT_RISK_PCT));
  const exposure = capitalNum * positionsNum;
  // Rust refuses to save or start a bot above the cap: every signal would be
  // skipped with "Above per-position cap". Said here, before Start.
  const overCap = capitalAboveCap(capitalNum, maxCapitalQuote);
  const riskOutOfRange = sizing === "risk" && !(riskPctNum >= MIN_RISK_PCT && riskPctNum <= MAX_RISK_PCT);
  const tpInvalid = !draftValid(takeProfit) || tpOverrides.some((o) => !draftValid(o));
  // An empty age field means the server's default, which must be known.
  const maxAgeMin =
    maxAgeH.trim() === "" ? (defaults?.maxSignalAgeMin ?? null) : Math.max(0, Math.round((parseDecimal(maxAgeH) || 0) * 60));
  const valid =
    seed !== "none" && capitalNum > 0 && !overCap && exchangeId !== "" && !riskOutOfRange && !tpInvalid && maxAgeMin !== null;
  const lossEffect =
    maxLoss.trim() === ""
      ? null
      : maxLossEffect({
          sizing,
          capPct: parseDecimal(maxLoss),
          leverage: leverageNum,
          riskPct: riskPctForPreview,
          feeRate: futuresMarket ? FUTURES_FEE_RATE : SPOT_FEE_RATE,
        });
  const symbolList = symbols
    .split(",")
    .map((s) => s.trim().toUpperCase())
    .filter(Boolean);
  const riskRange = {
    min: formatNumber(MIN_RISK_PCT, locale, { minimumFractionDigits: 1 }),
    max: formatNumber(MAX_RISK_PCT, locale, { minimumFractionDigits: 1 }),
  };
  const locked = running || busy;

  function buildConfig(): BotConfig {
    const confNum = parseDecimal(minConfidence);
    const maxLossNum = parseDecimal(maxLoss);
    // Combo and engine ids are lowercase on the wire ("stophunt_snap", "fvg").
    const csvLower = (raw: string) =>
      raw
        .split(",")
        .map((s) => s.trim().toLowerCase())
        .filter(Boolean);
    return {
      kind,
      exchangeId,
      maxPositions: positionsNum,
      capital: capitalNum,
      leverage: leverageNum,
      minConfidence: minConfidence !== "" && confNum > 0 ? confNum / 100 : undefined,
      direction: direction !== "all" ? direction : undefined,
      symbols: symbolList,
      combos,
      engines: csvLower(engines),
      maxLossPct: maxLoss !== "" && maxLossNum > 0 ? maxLossNum : undefined,
      sizing,
      riskPerTradePct: riskPctNum,
      takeProfit: targetFromDraft(takeProfit),
      takeProfitOverrides: Object.fromEntries(tpOverrides.map((o) => [o.symbol, targetFromDraft(o)])),
      // `valid` guarantees a number here. `live` is left out on purpose:
      // bot_configure ignores it, only bot_set_live changes it.
      maxSignalAgeMin: maxAgeMin ?? 0,
    };
  }

  // A running bot must always be stoppable: `disabled` / `!valid` gate STARTING only.
  const startOff = busy || disabled || !valid;
  // Live chosen: Start saves, shows the money at risk and asks for LIVE.
  const action = live ? signalStartAction(config, chosen, valid ? buildConfig() : null) : "start";
  const startButton = (
    <Button
      size="sm"
      variant={action === "confirmLive" ? "danger" : undefined}
      disabled={startOff || action === "toPaper"}
      disabledReason={disabled && disabledReasonKey ? t(disabledReasonKey) : undefined}
      tooltipAlign="end"
      onClick={() => (action === "confirmLive" && live ? live.onStartLive(buildConfig()) : onStart(buildConfig()))}
    >
      {t(action === "confirmLive" ? liveWordKey("startLive", live?.testnet === true) : "bots.start")}
    </Button>
  );

  return (
    <form
      className="ae-sform ae-sigform"
      // Enter in a field never starts a bot: Start is an explicit click.
      onSubmit={(e) => e.preventDefault()}
    >
      {defaultsError ? (
        <div className="ae-banner" data-tone="danger" role="alert">
          <span>{defaultsError}</span>
          <Button variant="secondary" size="sm" onClick={() => setDefaultsAttempt((n) => n + 1)}>
            {t("common.retry")}
          </Button>
        </div>
      ) : null}

      {live ? <ModePanel config={config} chosen={chosen} onChoose={setChosen} busy={busy} live={live} /> : null}

      <Panel title={t("strategy.form.section.general")}>
        <div className="ae-sform__grid">
          <FieldShell
            label={t("bots.exchange")}
            htmlFor={`${ids}-ex`}
            hint={
              exchangeLocked
                ? t("bots.exchangeLocked")
                : undefined
            }
          >
            <select
              id={`${ids}-ex`}
              className="ae-field__input"
              value={exchangeId}
              onChange={(e) => setExchangeSel(e.target.value)}
              disabled={locked || exchangeLocked || usable.length === 0}
            >
              {usable.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                </option>
              ))}
            </select>
          </FieldShell>
          <TextNum
            id={`${ids}-cap`}
            label={t("bots.capital")}
            value={capital}
            onChange={setCapital}
            disabled={locked}
            hint={
              capitalNum > 0 && capitalNum < EXCHANGE_MIN_ORDER_USDT
                ? t("bots.capitalBelowMinimum")
                : maxCapitalQuote != null
                  ? t("bots.capitalCapHint", { cap: formatUsdt(maxCapitalQuote, locale) })
                  : undefined
            }
            error={overCap && maxCapitalQuote != null ? t("bots.capitalAboveCap", { cap: formatUsdt(maxCapitalQuote, locale) }) : null}
          />
          <TextNum
            id={`${ids}-pos`}
            label={t("bots.maxPositions")}
            value={positions}
            onChange={setPositions}
            disabled={locked}
            integer
            hint={
              levelMaxPositions != null
                ? positionsNum > levelMaxPositions
                  ? t("bots.maxPositionsAboveLevel", { max: levelMaxPositions })
                  : t("bots.maxPositionsLevelHint", { max: levelMaxPositions })
                : undefined
            }
          />
          {futuresMarket ? (
            <TextNum
              id={`${ids}-lev`}
              label={t(sizing === "risk" ? "bots.leverageCapLabel" : "bots.leverage", { max: maxLeverage })}
              hint={sizing === "risk" ? t("bots.leverageCapHint") : undefined}
              value={leverage}
              onChange={setLeverage}
              disabled={locked}
              integer
              unit="x"
            />
          ) : null}
          <ReadOnlyField label={t("safety.exposure")} value={formatUsdt(exposure, locale)} numeric />
        </div>

        {/* Sizing changes what capital and leverage mean (a leverage that is
            the size in fixed mode becomes only a ceiling in risk mode). */}
        <div className="ae-sform__options">
          <FieldShell label={t("bots.sizing.title")}>
            <SegmentedControl
              label={t("bots.sizing.title")}
              value={sizing}
              fill
              wrap
              disabled={locked}
              onChange={setSizing}
              options={[
                { value: "risk", label: t("bots.sizing.risk") },
                { value: "fixed", label: t("bots.sizing.fixed") },
              ]}
            />
          </FieldShell>
          {sizing === "risk" ? (
            <TextNum
              id={`${ids}-risk`}
              label={t("bots.sizing.riskPct")}
              value={riskPct}
              onChange={setRiskPct}
              disabled={locked}
              unit="%"
              hint={t("bots.sizing.riskPctHint", riskRange)}
              error={riskOutOfRange ? t("bots.sizing.riskPctOutOfRange", riskRange) : null}
            />
          ) : null}
        </div>
        {sizing === "risk" ? <SizingPreview capital={capitalNum} leverageCap={leverageNum} riskPct={riskPctForPreview} /> : null}
      </Panel>

      <Panel title={t("bots.tp.title")}>
        <TakeProfitPicker
          value={takeProfit}
          onChange={setTakeProfit}
          overrides={tpOverrides}
          onOverridesChange={setTpOverrides}
          botSymbols={symbolList}
          exchangeId={exchangeId}
          futures={futuresMarket}
          disabled={locked}
          showLabel={false}
        />
      </Panel>

      <Panel title={t("bots.signalFilter")}>
        <TrendMismatchNote long={direction !== "short"} short={futuresMarket && direction !== "long"} />
        <div className="ae-sform__grid">
          <TextNum
            id={`${ids}-age`}
            label={t("bots.maxSignalAge")}
            placeholder={defaults?.maxSignalAgeMin != null ? formatDecimalInput(defaults.maxSignalAgeMin / 60) : ""}
            hint={t("bots.maxSignalAgeHint")}
            value={maxAgeH}
            onChange={setMaxAgeH}
            disabled={locked}
            unit="h"
          />
          <TextNum
            id={`${ids}-conf`}
            label={t("bots.minConfidence")}
            placeholder="0"
            hint={t("bots.minConfidenceHint")}
            value={minConfidence}
            onChange={setMinConfidence}
            disabled={locked}
            unit="%"
          />
          <FieldShell label={t("bots.direction")}>
            <SegmentedControl
              label={t("bots.direction")}
              value={direction}
              fill
              wrap
              disabled={locked || kind === "spot"}
              onChange={setDirection}
              options={[
                { value: "all", label: t("bots.dir.all") },
                { value: "long", label: t("bots.dir.long") },
                {
                  value: "short",
                  label: t("bots.dir.short"),
                  disabled: !futuresMarket,
                  reason: t("bots.skipReasons.spotLongOnly"),
                },
              ]}
            />
          </FieldShell>
          {/* No placeholder number: an empty field is off, and a grey "2"
              read as an active default that was never applied. */}
          <TextNum
            id={`${ids}-loss`}
            label={t("bots.maxLossPct")}
            hint={
              lossEffect
                ? `${t("bots.maxLossPctHint")} · ${maxLossText(t, lossEffect, riskPctForPreview, leverageNum, locale)}`
                : t("bots.maxLossPctHint")
            }
            value={maxLoss}
            onChange={setMaxLoss}
            disabled={locked}
            unit="%"
          />
          <FieldShell label={t("bots.symbolWhitelist")} htmlFor={`${ids}-sym`} hint={t("bots.symbolWhitelistHint")}>
            <input
              id={`${ids}-sym`}
              className="ae-field__input mono"
              placeholder="BTCUSDT, ETHUSDT"
              value={symbols}
              onChange={(e) => setSymbols(e.target.value)}
              disabled={locked}
              autoComplete="off"
              spellCheck={false}
            />
          </FieldShell>
          <FieldShell label={t("bots.engineWhitelist")} htmlFor={`${ids}-eng`} hint={t("bots.engineWhitelistHint")}>
            <input
              id={`${ids}-eng`}
              className="ae-field__input mono"
              placeholder="fvg, ob, msb"
              value={engines}
              onChange={(e) => setEngines(e.target.value)}
              disabled={locked}
              autoComplete="off"
              spellCheck={false}
            />
          </FieldShell>
        </div>
        <div className="ae-sform__options ae-sigform__combos">
          <ComboPicker value={combos} onChange={setCombos} disabled={locked} />
        </div>
        <p className="ae-field__hint">{t("bots.filtersApplyPending")}</p>
      </Panel>

      <div className="ae-sform__actions">
        <span className="ae-toolbar__spacer" />
        {running ? (
          <Button variant="secondary" size="sm" disabled={busy} onClick={onStop}>
            {t("bots.stop")}
          </Button>
        ) : (
          startButton
        )}
      </div>
    </form>
  );
}

/** What the typed max-loss cap does under the current sizing (lib/sizing.ts). */
function maxLossText(
  t: TFunction,
  e: MaxLossEffect,
  riskPct: number,
  leverage: number,
  locale: string,
): string {
  switch (e.kind) {
    case "fixed":
      return t("bots.maxLoss.fixed", { move: formatPercent(e.movePct, locale) });
    case "immediate":
      return t("bots.maxLoss.immediate", { leverage });
    case "riskInert":
      return t("bots.maxLoss.riskInert", { risk: formatNumber(riskPct, locale, { maximumFractionDigits: 2 }) });
    case "riskActive":
      return t("bots.maxLoss.riskActive", { share: formatPercent(e.stopShare * 100, locale, 0) });
  }
}

function overridesFrom(config: BotConfig | null | undefined): OverrideDraft[] {
  return Object.entries(config?.takeProfitOverrides ?? {}).map(([symbol, target]) => ({
    symbol,
    ...draftFromTarget(target),
  }));
}

interface TextNumProps {
  id: string;
  label: string;
  value: string;
  onChange: (v: string) => void;
  disabled?: boolean;
  integer?: boolean;
  unit?: string;
  placeholder?: string;
  hint?: ReactNode;
  error?: string | null;
}

/** A number typed as text (so "1." can exist mid-edit), in the form's field shell. */
function TextNum({ id, label, value, onChange, disabled, integer, unit, placeholder, hint, error }: TextNumProps) {
  return (
    <FieldShell label={label} htmlFor={id} hint={hint} error={error}>
      <span className="ae-sfield__input">
        <input
          id={id}
          className="ae-field__input mono"
          type="text"
          inputMode={integer ? "numeric" : "decimal"}
          placeholder={placeholder}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          disabled={disabled}
          aria-invalid={error ? true : undefined}
          autoComplete="off"
        />
        {unit ? <span className="ae-sfield__unit">{unit}</span> : null}
      </span>
    </FieldShell>
  );
}

interface PreviewRow extends SizeEstimate {
  stopFrac: number;
}

/**
 * The hypothetical stops in EXAMPLE_STOP_FRACS, sized with the SAME formula
 * as src-tauri/src/bot/engine/sizing.rs, for preview only. The server sizes
 * every real signal from ITS actual entry and stop.
 */
function SizingPreview({ capital, leverageCap, riskPct }: { capital: number; leverageCap: number; riskPct: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const rows: PreviewRow[] = EXAMPLE_STOP_FRACS.map((stopFrac) => ({
    stopFrac,
    ...estimateRiskSize(capital, leverageCap, riskPct, stopFrac),
  }));
  const columns: DataColumn<PreviewRow>[] = [
    { id: "sl", header: t("table.sl"), cell: (r) => formatPercent(r.stopFrac * 100, locale, 0) },
    { id: "notional", header: t("table.notional"), numeric: true, cell: (r) => formatUsdt(r.notional, locale, 0) },
    {
      id: "lev",
      header: t("table.leverage"),
      numeric: true,
      priority: 2,
      cell: (r) => (
        <span className="ae-namecell">
          {r.riskCapped ? (
            <Chip tone="warn" title={t("bots.sizing.cappedNote")}>
              {t("bots.position.riskCappedBadge")}
            </Chip>
          ) : null}
          {`${formatNumber(r.effectiveLeverage, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })}x`}
        </span>
      ),
    },
    {
      id: "loss",
      header: t("table.pnlUsdt"),
      numeric: true,
      cell: (r) => formatSignedUsdt(-r.costUsdt, locale),
      tone: () => "down",
    },
    { id: "pct", header: t("table.pnlPct"), numeric: true, priority: 2, cell: (r) => formatSignedPercent(-r.costPctOfCapital, locale), tone: () => "down" },
  ];
  return (
    <div className="ae-sigform__preview">
      <DataTable
        label={t("bots.sizing.exampleTitle")}
        columns={columns}
        rows={rows}
        rowKey={(r) => String(r.stopFrac)}
        compact
        breakpoints={{ p2: 380, p3: 380 }}
      />
    </div>
  );
}

/**
 * The bot's mode: Paper or Live, chosen here. Paper is the default and Live
 * is never pre-selected. Choosing Live does nothing by itself: Start then
 * saves the settings, shows the money at risk and asks for the typed LIVE
 * (`onStartLive`). A Live bot comes back stopped after every restart and
 * asks for LIVE again. Back to Paper stops the bot; it becomes Paper once it
 * holds no real position. Rust re-checks every condition; its refusal is
 * shown as-is.
 */
export function ModePanel({
  config,
  chosen,
  onChoose,
  busy,
  live,
}: {
  config: BotConfig | null;
  chosen: TradeMode;
  onChoose: (m: TradeMode) => void;
  busy: boolean;
  live: BotLiveControl;
}) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const saved = botMode(config);
  const armed = config?.live === true;
  const [toPaper, setToPaper] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const shown = live.error ?? error;

  const pick = (m: TradeMode) => {
    setError(null);
    if (m === "paper" && saved === "live") setToPaper(true);
    else onChoose(m);
  };

  const confirmPaper = async () => {
    const err = await live.onToPaper();
    setToPaper(false);
    if (err) setError(err);
    else onChoose("paper");
  };

  // Real positions open: name them, and offer to close them first.
  const held = live.realPositions;
  const closeAndSwitch = async () => {
    const err = await live.onCloseAndPaper();
    setToPaper(false);
    if (err) setError(err);
    else onChoose("paper");
  };

  const note =
    saved === "live" && armed
      ? live.pilotLeft > 0
        ? t("bots.live.pilot", { count: live.pilotLeft, cap: formatNumber(PILOT_NOTIONAL_USDT, locale) })
        : t("liveMode.signal.armed")
      : saved === "live"
        ? t("liveMode.signal.needsArm")
        : t("liveMode.signal.chooseLive");

  return (
    <Panel title={t("liveMode.label")} tone={chosen === "live" ? "live" : "default"} aside={saved === "live" ? <LiveChip /> : <PaperChip />}>
      <ModePicker
        value={chosen}
        onChange={pick}
        // A Live bot keeps Live selectable: Start checks its arm.
        blockers={saved === "live" ? [] : live.blockers}
        disabled={busy}
        liveNote={note}
      />
      {saved === "live" && !armed && live.blockers.length > 0 ? (
        <ul className="ae-livemode__blockers">
          {live.blockers.map((b) => (
            <li key={b.key}>{t(b.key, b.params)}</li>
          ))}
        </ul>
      ) : null}
      <div className="ae-toolbar">
        {saved === "live" && armed && live.pilotLeft > 0 ? (
          <Button variant="ghost" size="xs" disabled={busy} onClick={() => void live.onEndPilot().then((e) => e && setError(e))}>
            {t("bots.live.endPilot")}
          </Button>
        ) : null}
        <Link to="/risk?s=preflight" className="ae-link">
          {t("preflight.title")}
        </Link>
      </div>
      {shown ? (
        <p className="ae-sfield__error" role="alert">
          {shown}
        </p>
      ) : null}
      <ConfirmDialog
        open={toPaper && held.length === 0}
        danger
        title={t("liveMode.signal.toPaperTitle")}
        body={<p>{t("liveMode.signal.toPaperBody")}</p>}
        confirmLabel={t("bots.live.switchOff")}
        busy={busy}
        onConfirm={() => void confirmPaper()}
        onCancel={() => setToPaper(false)}
      />
      <ConfirmDialog
        open={toPaper && held.length > 0}
        live
        danger
        title={t("liveMode.signal.toPaperTitle")}
        body={
          <>
            <p>{t("liveMode.signal.toPaperHeld", { count: held.length })}</p>
            <ul className="ae-livemode__held">
              {held.map((p) => (
                <li key={p.signalId} className="tabular">
                  {t("liveMode.signal.heldLine", {
                    symbol: p.symbol,
                    side: t(`signalDesk.direction.${p.direction === "short" ? "short" : "long"}`),
                    qty: formatNumber(p.qty, locale, { maximumFractionDigits: 8 }),
                    entry: formatNumber(p.entry, locale, { maximumFractionDigits: 8 }),
                  })}
                </li>
              ))}
            </ul>
            <p>{t("liveMode.signal.closeAndSwitchNote")}</p>
          </>
        }
        word={CLOSE_WORD}
        confirmLabel={t("liveMode.signal.closeAndSwitch")}
        alternative={{ label: t("liveMode.signal.switchLater"), onSelect: () => void confirmPaper() }}
        busy={busy}
        onConfirm={() => void closeAndSwitch()}
        onCancel={() => setToPaper(false)}
      />
    </Panel>
  );
}
