import { useTranslation } from "react-i18next";
import { Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatUsdt } from "@/lib/format";
import { PINE_LIMITS, PINE_TIMEFRAMES } from "@/lib/ipc/pine/pine";
import { FieldShell, NumField, Segmented } from "@/pages/Bots/strategy/formControls";
import { budgetFloor, marginPerPosition, parseAllowList } from "@/pages/Bots/tradingview/tvBotModel";
import type { PineBotForm } from "./pineBotModel";
import { pairsBelowMinimum, usePairMinimums } from "./usePairMinimums";

interface Props {
  form: PineBotForm;
  set: (patch: Partial<PineBotForm>) => void;
  invalid: (key: string) => (bad: boolean) => void;
  maxLeverage: number | null;
  /** Numeric inputs of the script (by title): the stop / target may be read from one. */
  numericInputs: string[];
  /** Live chosen: the bot's own stop is required. */
  live: boolean;
}

/** Market (pairs, timeframe, warm-up bars), money and exits, and the guards of the Pine bot. */
export function PineSettingsPanels({ form, set, invalid, maxLeverage, numericInputs, live }: Props) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const L = PINE_LIMITS;
  const k = (x: string) => `botsPine.${x}`;
  const usd2 = (v: number) => formatNumber(v, locale, { maximumFractionDigits: 2 });
  const floor = budgetFloor(form, maxLeverage);
  const budgetBelow = floor !== null && form.budgetUsdt > 0 && form.budgetUsdt < floor.min - 1e-9;
  const pairs = parseAllowList(form.symbols).slice(0, L.maxSymbols);
  const mins = usePairMinimums(pairs);
  const below = pairsBelowMinimum(mins, form.maxSizeUsdt);
  const minList = pairs.filter((s) => mins[s] !== undefined).map((s) => `${s} ${usd2(mins[s])}`);
  const floorParams = floor ? { min: usd2(floor.min), size: usd2(form.maxSizeUsdt), leverage: floor.leverage, positions: form.maxPositions, full: usd2(floor.full) } : null;
  const stopMissing = live && !(form.stopPct > 0);

  const inputSelect = (id: string, label: string, value: string, onChange: (v: string) => void, hint: string) => (
    <FieldShell label={label} htmlFor={id} hint={hint}>
      <select id={id} className="ae-field__input" value={value} disabled={numericInputs.length === 0 && !value} onChange={(e) => onChange(e.target.value)}>
        <option value="">{t(k("fromInputNone"))}</option>
        {[...new Set([...numericInputs, ...(value ? [value] : [])])].map((n) => (
          <option key={n} value={n}>
            {n}
          </option>
        ))}
      </select>
    </FieldShell>
  );

  return (
    <>
      <Panel title={t(k("market"))}>
        <div className="ae-sform__grid">
          <FieldShell label={t(k("symbols"))} htmlFor="pine-symbols" hint={t(k("symbolsHint"), { max: L.maxSymbols })}>
            <input id="pine-symbols" className="ae-field__input mono" value={form.symbols} placeholder="BTCUSDT, ETHUSDT" onChange={(e) => set({ symbols: e.target.value })} />
          </FieldShell>
          <Segmented
            label={t(k("timeframe"))}
            value={form.timeframe}
            onChange={(v) => set({ timeframe: v })}
            options={PINE_TIMEFRAMES.map((tf) => ({ value: tf, label: tf }))}
            hint={t(k("timeframeHint"))}
          />
          <NumField
            label={t(k("warmup"))}
            integer
            wireMax={100_000}
            value={form.warmupBars}
            onChange={(v) => set({ warmupBars: v })}
            onInvalid={invalid("warmup")}
            invalidText={t(k("error.warmup"), { min: L.warmupMin, max: L.warmupMax })}
            hint={t(k("warmupHint"), { min: L.warmupMin, max: L.warmupMax })}
          />
        </div>
      </Panel>

      <Panel title={t(k("money"))}>
        <div className="ae-sform__grid">
          <NumField
            label={t(k("budget"))}
            unit="USDT"
            value={form.budgetUsdt}
            onChange={(v) => set({ budgetUsdt: v })}
            onInvalid={invalid("budget")}
            invalidText={t(k("error.budget"))}
            error={budgetBelow && floorParams ? t(k("error.budgetBelow"), floorParams) : null}
            hint={
              <>
                {floorParams ? <span className="ae-sfield__line">{t(k("budgetMin"), floorParams)}</span> : null}
                <span className="ae-sfield__line">{t(k("budgetHint"), { margin: formatUsdt(marginPerPosition(form), locale) })}</span>
              </>
            }
          />
          <NumField
            label={t(k("maxSize"))}
            unit="USDT"
            value={form.maxSizeUsdt}
            onChange={(v) => set({ maxSizeUsdt: v })}
            onInvalid={invalid("size")}
            invalidText={t(k("error.size"), { min: L.sizeMin })}
            error={below.length ? t(k("sizeBelowMin"), { pairs: below.map((b) => b.symbol).join(", "), min: usd2(below[0].min) }) : null}
            hint={
              <>
                <span className="ae-sfield__line">{t(k("maxSizeHint"), { min: L.sizeMin })}</span>
                {minList.length ? <span className="ae-sfield__line">{t(k("pairMins"), { list: minList.join(" · ") })}</span> : null}
              </>
            }
          />
          <NumField label={t("bots.maxPositions")} integer wireMax={20} value={form.maxPositions} onChange={(v) => set({ maxPositions: v })} onInvalid={invalid("positions")} invalidText={t(k("error.positions"))} />
          <NumField
            label={t("botsTv.leverage")}
            unit="x"
            integer
            wireMax={125}
            value={form.leverage}
            onChange={(v) => set({ leverage: v })}
            onInvalid={invalid("leverage")}
            invalidText={t(k("error.leverage"), { max: maxLeverage ?? 1 })}
            hint={maxLeverage !== null ? t("botsTv.leverageHint", { max: maxLeverage }) : undefined}
          />
          <NumField
            label={t(k("opensPerDay"))}
            integer
            wireMax={L.opensMax}
            value={form.maxOpensPerDay}
            onChange={(v) => set({ maxOpensPerDay: v })}
            onInvalid={invalid("opens")}
            invalidText={t(k("error.opens"), { max: L.opensMax })}
            hint={t(k("opensHint"))}
          />
          <NumField
            label={t(k("stop"))}
            unit="%"
            value={form.stopPct}
            onChange={(v) => set({ stopPct: v })}
            onInvalid={invalid("stop")}
            invalidText={t(k("error.stop"), { min: L.stopMin, max: L.stopMax })}
            error={stopMissing ? t(k("error.liveStop")) : null}
            hint={t(k("stopHint"), { min: L.stopMin, max: L.stopMax })}
          />
          <NumField
            label={t(k("target"))}
            unit="%"
            value={form.takeProfitPct}
            onChange={(v) => set({ takeProfitPct: v })}
            onInvalid={invalid("target")}
            invalidText={t(k("error.target"), { min: L.targetMin, max: L.targetMax })}
            hint={t(k("targetHint"), { min: L.targetMin, max: L.targetMax })}
          />
          {inputSelect("pine-stopin", t(k("stopInput")), form.stopInput, (v) => set({ stopInput: v }), t(k("stopInputHint")))}
          {inputSelect("pine-targetin", t(k("targetInput")), form.targetInput, (v) => set({ targetInput: v }), t(k("targetInputHint")))}
        </div>
      </Panel>

      <Panel title={t(k("guards"))}>
        <div className="ae-sform__grid">
          <NumField
            label={t(k("stale"))}
            unit="s"
            integer
            wireMax={L.staleMax * 10}
            value={form.staleS}
            onChange={(v) => set({ staleS: v })}
            onInvalid={invalid("stale")}
            invalidText={t(k("error.stale"), { min: L.staleMin, max: L.staleMax })}
            hint={t(k("staleHint"), { min: L.staleMin, max: L.staleMax })}
          />
          <NumField
            label={t(k("priceGuard"))}
            unit="%"
            value={form.priceGuardPct}
            onChange={(v) => set({ priceGuardPct: v })}
            onInvalid={invalid("guard")}
            invalidText={t(k("error.guard"), { max: L.guardMax })}
            hint={t(k("priceGuardHint"))}
          />
        </div>
      </Panel>
    </>
  );
}
