import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip, PaperChip } from "@/components/ui/Chip/Chip";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatUsdt, NO_VALUE } from "@/lib/format";
import { botDefaultConfig, botMode, TV_LIMITS, tvPassphraseSet, type BotConfig, type TradeMode } from "@/lib/ipc/bot/bot";
import { errorMessage } from "@/lib/ipc/bridge";
import { liveWordKey, ordersOnTestnet, signalStartAction } from "@/lib/liveMode";
import { PositionsTable } from "@/pages/Positions/PositionsTable";
import { Section } from "@/components/ui/Section/Section";
import { SymbolMinimumField } from "./SymbolMinimumField";
import { CheckRow, FieldShell, NumField, Segmented } from "@/pages/Bots/strategy/formControls";
import { ModePanel } from "@/pages/Bots/signal/SignalBotForm";
import { useSignalLive } from "@/pages/Bots/signal/useSignalLive";
import "@/pages/Bots/strategy/StrategyForm.css";
import "@/pages/Bots/bots.css";
import { budgetFloor, configFromForm, formError, formFromConfig, marginPerPosition, type TvBotForm } from "./tvBotModel";
import { formatDecimalInput } from "@/lib/decimal";

const TV_SETTINGS = "/settings?tab=tradingview";

/**
 * #/bots/tradingview: the TradingView bot, which trades the user's own
 * alerts (Settings, TradingView) and never Sentinel's signals. Paper by
 * default; Live on Binance USDT-M futures with the typed LIVE every session.
 * Its limits are enforced by Rust on every alert; this form only writes them.
 */
export function TvBotPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk, catalog } = useDeskContext();
  const s = desk.status;
  const cfg = desk.loaded ? (s.tradingview ?? null) : null;
  const running = s.tradingviewRunning === true;
  const exchanges = (catalog.exchanges ?? []).filter((e) => e.supportsFutures);
  const maxLeverage = risk.state?.limits.maxLeverage ?? null;
  const [form, setForm] = useState<TvBotForm>(() => formFromConfig(cfg, "binance"));
  const [chosen, setChosen] = useState<TradeMode>(botMode(cfg));
  const [bad, setBad] = useState<Record<string, boolean>>({});
  const [base, setBase] = useState<BotConfig | null>(cfg);
  const [error, setError] = useState<string | null>(null);
  const live = useSignalLive("tradingview");

  // The stored bot (or the server's defaults for a new one) is the base;
  // re-seeded only when the stored bot itself changes.
  const cfgKey = JSON.stringify(cfg);
  useEffect(() => {
    if (cfg) {
      setBase(cfg);
      setForm(formFromConfig(cfg, "binance"));
      setChosen(botMode(cfg));
      return;
    }
    if (!desk.loaded) return;
    let gone = false;
    botDefaultConfig("tradingview", "binance")
      .then((d) => !gone && setBase(d))
      .catch((e) => !gone && setError(errorMessage(e, t("bots.defaultsFailed"))));
    return () => {
      gone = true;
    };
  }, [desk.loaded, cfgKey]);

  const set = (patch: Partial<TvBotForm>) => setForm((f) => ({ ...f, ...patch }));
  const invalid = (k: string) => (v: boolean) => setBad((b) => ({ ...b, [k]: v }));
  const formErr = formError(form, maxLeverage);
  // The budget one max-size alert needs (the alert guard's own rule).
  const floor = budgetFloor(form, maxLeverage);
  const usd2 = (v: number) => formatNumber(v, locale, { maximumFractionDigits: 2 });
  const floorParams = floor ? { min: usd2(floor.min), size: usd2(form.maxSizeUsdt), leverage: floor.leverage, positions: form.maxPositions, full: usd2(floor.full) } : null;
  const budgetBelow = floor !== null && form.budgetUsdt > 0 && form.budgetUsdt < floor.min - 1e-9;
  const anyBad = Object.values(bad).some(Boolean);
  const next = base ? configFromForm(form, base) : null;
  const action = signalStartAction(cfg, chosen, next);
  const k = (x: string) => `botsTv.${x}`;
  const canSave = !!next && !formErr && !anyBad && !desk.busy;
  const exchangeLocked = botMode(cfg) === "live" || s.openPositions.some((p) => p.botKind === "tradingview" && p.live);

  const save = async () => {
    if (!next) return;
    setError(null);
    const err = await desk.configure(next);
    if (err) setError(err);
    else set({ passphraseAction: "keep", passphrase: "" });
  };

  const start = async () => {
    if (!next) return;
    setError(null);
    if (action === "confirmLive") return live.control.onStartLive(next);
    await desk.configureAndStart(next);
    set({ passphraseAction: "keep", passphrase: "" });
  };

  const mine = s.openPositions.filter((p) => p.botKind === "tradingview");
  const marginUsed = mine.reduce((n, p) => n + p.capital, 0);

  const right = (
    <>
      <Panel title={t(k("now"))} aside={botMode(cfg) === "live" ? <LiveChip /> : cfg ? <PaperChip /> : null}>
        <FactList
          rows={[
            { label: t(k("state")), value: !cfg ? t(k("notSaved")) : running ? t("settings.tv.botRunning") : t("settings.tv.botStopped") },
            { label: t("table.openPositions"), value: cfg ? `${mine.length} / ${cfg.maxPositions}` : NO_VALUE },
            {
              label: t(k("marginUsed")),
              value: cfg?.tv ? `${formatUsdt(marginUsed, locale)} / ${formatUsdt(cfg.tv.budgetUsdt, locale)}` : NO_VALUE,
            },
            ...(botMode(cfg) === "live" && cfg?.live
              ? [{ label: t(k("pilot")), value: formatNumber(s.tradingviewPilotLeft ?? 0, locale) }]
              : []),
            { label: t(k("passphrase")), value: tvPassphraseSet(cfg?.tv) ? t(k("passphraseSet")) : t(k("passphraseNone")) },
          ]}
        />
        <p className="ae-newbot__links">
          <Link to={TV_SETTINGS} className="ae-link">
            {t(k("toSettings"))}
          </Link>
        </p>
      </Panel>
      <Panel title={t(k("rulesTitle"))}>
        <ul className="ae-setlist">
          {["rule1", "rule2", "rule3", "rule4"].map((r) => (
            <li key={r}>{t(k(r))}</li>
          ))}
        </ul>
      </Panel>
    </>
  );

  const primary = running ? (
    <Button size="sm" variant="secondary" disabled={desk.busy} onClick={() => void desk.stop("tradingview")}>
      {t("bots.stop")}
    </Button>
  ) : (
    <Button size="sm" variant={chosen === "live" ? "danger" : "primary"} disabled={!canSave || s.killSwitchTripped} onClick={() => void start()}>
      {chosen === "live" ? t(liveWordKey("startLive", ordersOnTestnet(s, desk.loaded))) : t("bots.start")}
    </Button>
  );

  return (
    <PageShell
      title={t("page.tvBot")}
      crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]}
      right={right}
      primary={primary}
      secondary={
        <Button size="sm" variant="secondary" disabled={!canSave} onClick={() => void save()}>
          {t(k("save"))}
        </Button>
      }
    >
      <ModePanel config={cfg} chosen={chosen} onChoose={setChosen} busy={desk.busy} live={live.control} />
      {live.dialog}

      {mine.length > 0 ? (
        <Section title={t("table.openPositions")} count={mine.length}>
          <PositionsTable positions={mine} emptyKey="positions.noOpen" onClose={desk.closePosition} />
        </Section>
      ) : null}

      <Panel title={t(k("general"))}>
        <div className="ae-sform__grid">
          <FieldShell label={t(k("name"))} htmlFor="tvb-name">
            <input id="tvb-name" className="ae-field__input" value={form.name} maxLength={40} onChange={(e) => set({ name: e.target.value })} />
          </FieldShell>
          <FieldShell label={t("bots.exchange")} htmlFor="tvb-ex" hint={exchangeLocked ? t("bots.exchangeLocked") : t(k("exchangeHint"))}>
            <select
              id="tvb-ex"
              className="ae-field__input"
              value={form.exchangeId}
              disabled={exchangeLocked || exchanges.length === 0}
              onChange={(e) => set({ exchangeId: e.target.value })}
            >
              {exchanges.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                </option>
              ))}
            </select>
          </FieldShell>
        </div>
      </Panel>

      <Panel title={t(k("limits"))}>
        <div className="ae-sform__grid">
          <NumField
            label={t(k("budget"))}
            unit="USDT"
            value={form.budgetUsdt}
            onChange={(v) => set({ budgetUsdt: v })}
            onInvalid={invalid("budget")}
            invalidText={floorParams ? `${t("strategy.form.notANumber")} ${t("strategy.budgetFloor.minShort", floorParams)}` : t(k("error.budget"))}
            emptyText={floorParams ? t(k("error.budgetEmpty"), floorParams) : t("strategy.budgetFloor.emptyNoMin")}
            placeholder={floor ? formatDecimalInput(floor.min) : undefined}
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
            invalidText={t(k("error.size"), { min: TV_LIMITS.sizeMin })}
            emptyText={t(k("error.sizeEmpty"), { min: TV_LIMITS.sizeMin })}
            placeholder={String(TV_LIMITS.sizeMin)}
            hint={`${t(k("maxSizeHint"))} ${t(k("maxSizeMin"), { min: TV_LIMITS.sizeMin })}`}
          />
          <NumField label={t("bots.maxPositions")} integer wireMax={20} value={form.maxPositions} onChange={(v) => set({ maxPositions: v })} onInvalid={invalid("positions")} invalidText={t(k("error.positions"))} />
          <NumField
            label={t(k("leverage"))}
            unit="x"
            integer
            wireMax={125}
            value={form.leverage}
            onChange={(v) => set({ leverage: v })}
            onInvalid={invalid("leverage")}
            invalidText={t(k("error.leverage"), { max: maxLeverage ?? 1 })}
            hint={maxLeverage !== null ? t(k("leverageHint"), { max: maxLeverage }) : undefined}
          />
          <NumField label={t(k("opensPerDay"))} integer wireMax={200} value={form.maxOpensPerDay} onChange={(v) => set({ maxOpensPerDay: v })} onInvalid={invalid("opens")} invalidText={t(k("error.opens"), { max: 200 })} hint={t(k("opensHint"))} />
          <FieldShell label={t(k("allowList"))} htmlFor="tvb-allow" hint={t(k("allowListHint"))}>
            <input id="tvb-allow" className="ae-field__input mono" value={form.allowList} placeholder={t(k("allowListEmpty"))} onChange={(e) => set({ allowList: e.target.value })} />
          </FieldShell>
          {form.exchangeId === "binance" ? <SymbolMinimumField maxSizeUsdt={form.maxSizeUsdt} /> : null}
        </div>
      </Panel>

      <Panel title={t(k("guards"))}>
        <div className="ae-sform__grid">
          <NumField label={t(k("stale"))} unit="s" integer wireMax={120} value={form.staleS} onChange={(v) => set({ staleS: v })} onInvalid={invalid("stale")} invalidText={t(k("error.stale"), { min: 5, max: 120 })} hint={t(k("staleHint"))} />
          <NumField label={t(k("priceGuard"))} unit="%" value={form.priceGuardPct} onChange={(v) => set({ priceGuardPct: v })} onInvalid={invalid("guard")} invalidText={t(k("error.guard"), { max: 10 })} hint={t(k("priceGuardHint"))} />
          <CheckRow label={t(k("requirePrice"))} checked={form.requirePrice} onChange={(v) => set({ requirePrice: v })} hint={chosen === "live" && !form.requirePrice ? t(k("requirePriceLive")) : undefined} />
          <Segmented
            label={t(k("passphrase"))}
            value={form.passphraseAction}
            onChange={(v) => set({ passphraseAction: v })}
            options={[
              { value: "keep", label: t(tvPassphraseSet(cfg?.tv) ? k("passKeep") : k("passNone")) },
              { value: "set", label: t(k("passSet")) },
              { value: "clear", label: t(k("passClear")), disabled: !tvPassphraseSet(cfg?.tv) },
            ]}
            hint={t(k("passphraseHint"))}
          />
          {form.passphraseAction === "set" ? (
            <FieldShell label={t(k("passNew"))} htmlFor="tvb-pass">
              <input id="tvb-pass" className="ae-field__input mono" type="password" autoComplete="new-password" value={form.passphrase} onChange={(e) => set({ passphrase: e.target.value })} />
            </FieldShell>
          ) : null}
        </div>
      </Panel>

      {formErr ? (
        <p className="ae-sfield__error" role="alert">
          {t(k(`error.${formErr.key}`), formErr.params)}
        </p>
      ) : null}
      {error ? (
        <p className="ae-sfield__error" role="alert">
          {error}
        </p>
      ) : null}
    </PageShell>
  );
}
