import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { Button } from "@/components/ui/Button/Button";
import { Chip, LiveChip, PaperChip } from "@/components/ui/Chip/Chip";
import { LiveConfirmDialog, ModePicker } from "@/components/LiveMode/LiveMode";
import { SegmentedControl } from "@/components/ui/SegmentedControl/SegmentedControl";
import { TextField } from "@/components/ui/TextField/TextField";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPercent, formatPrice, formatSignedUsdt, formatUsdt, NO_VALUE } from "@/lib/format";
import {
  botMode,
  botOpenTrade,
  botPreviewTrade,
  PILOT_NOTIONAL_USDT,
  type BotDeskStatus,
  type LiveTerms,
  type TradeMode,
  type TradePreview,
  type TradeRequest,
} from "@/lib/ipc/bot/bot";
import type { Blocker } from "@/lib/liveMode";
import { EXCHANGE_MIN_ORDER_USDT } from "@/lib/botLimits";
import { exchangeSymbols } from "@/lib/ipc/exchange/exchange";
import type { SignalDirection } from "@/lib/ipc/signal/signal";
import { takeRefusalText } from "@/pages/Signals/TakeNow";
import { PortfolioWhatIf, tradeCandidate } from "@/pages/Risk/PortfolioWhatIf";
import "./NewTrade.css";

/** The signal bots a New trade can open on (Pump is refused). */
export type TradeBot = "futures" | "spot";
const BOTS: TradeBot[] = ["futures", "spot"];

/** What the user typed, as typed. */
export interface TradeForm {
  /** Paper (default) or Live: Live opens a real order on the armed Live Futures bot. */
  mode: TradeMode;
  kind: TradeBot;
  symbol: string;
  direction: SignalDirection;
  /** "bot": the bot's own sizing; "usdt": `notional`. */
  sizing: "bot" | "usdt";
  notional: string;
  takeProfit: string;
  stopLoss: string;
}

/** Wait this long after the last keystroke before asking for a preview. */
export const PREVIEW_DEBOUNCE_MS = 350;

function num(raw: string): number {
  const s = raw.trim().replace(",", ".");
  return s === "" ? Number.NaN : Number(s);
}

/**
 * The request for `form`, `"incomplete"` while a required field is empty,
 * or a refusal code for a value that is not a number (Rust's own codes).
 */
export function tradeRequest(form: TradeForm): TradeRequest | "incomplete" | string {
  const symbol = form.symbol.trim().toUpperCase();
  const blank = (s: string) => s.trim() === "";
  if (!symbol || blank(form.takeProfit) || blank(form.stopLoss) || (form.sizing === "usdt" && blank(form.notional))) {
    return "incomplete";
  }
  const takeProfit = num(form.takeProfit);
  const stopLoss = num(form.stopLoss);
  if (!(takeProfit > 0 && stopLoss > 0)) return "tradeLevelsInvalid";
  const req: TradeRequest = { kind: form.kind, symbol, direction: form.direction, takeProfit, stopLoss };
  if (form.mode === "live") req.mode = "live";
  if (form.sizing === "usdt") {
    const n = num(form.notional);
    if (!(n > 0)) return "tradeSizeInvalid";
    req.notionalUsdt = n;
  }
  return req;
}

/** A refusal of `bot_preview_trade` / `bot_open_trade`: the New trade wording first, then Take's chain. */
export function tradeRefusalText(t: TFunction, i18nExists: (key: string) => boolean, raw: string): string {
  const [head, ...rest] = raw.split("|");
  const own = `positions.newTrade.refusal.${head.trim()}`;
  if (i18nExists(own)) return t(own, { detail: rest.join("|").trim() });
  return takeRefusalText(t, i18nExists, raw);
}

function rawCode(e: unknown): string {
  if (typeof e === "string" && e.trim()) return e;
  if (e instanceof Error && e.message) return e.message;
  return "storeReadFailed";
}

export type TradeState =
  | { state: "incomplete" }
  | { state: "loading" }
  | { state: "refused"; code: string }
  | { state: "ready"; preview: TradePreview; busy: boolean }
  | { state: "opened"; symbol: string; entry: number };

/** The bot a New trade starts on: the first saved Paper one, Futures when none is. */
function firstBot(status: BotDeskStatus): TradeBot {
  return BOTS.find((k) => status[k] !== null && botMode(status[k]) === "paper") ?? "futures";
}

/**
 * Why a Live New trade cannot be chosen now (pure). It opens on the Futures
 * bot only, which must be a Live bot armed this session (the typed LIVE on
 * its settings); Rust re-checks both and the typed word.
 */
export function liveTradeBlockers(status: BotDeskStatus): Blocker[] {
  const out: Blocker[] = [];
  if (status.tradingBlocked) out.push({ key: "liveMode.blockers.tradingBlocked" });
  const fut = status.futures;
  if (botMode(fut) !== "live") out.push({ key: "liveMode.blockers.tradeBotPaper" });
  else if (fut?.live !== true) out.push({ key: "liveMode.blockers.notArmed" });
  return out;
}

/** The figures a Live New trade's confirmation shows, from the preview (Rust sizes the same request). */
export function tradeTerms(p: TradePreview, venue: string, pilotLeft: number): LiveTerms {
  const pilot = pilotLeft > 0;
  return {
    venue,
    marginAtRiskUsdt: p.capital,
    maxNotionalUsdt: pilot ? Math.min(p.notionalUsdt, PILOT_NOTIONAL_USDT) : p.notionalUsdt,
    leverage: p.leverage,
    positions: 1,
    pilotCount: pilotLeft,
    pilotNotionalUsdt: PILOT_NOTIONAL_USDT,
  };
}

export interface NewTradeController {
  form: TradeForm;
  set: (patch: Partial<TradeForm>) => void;
  trade: TradeState;
  /** `confirmation`: the typed word, for a Live trade. */
  open: (confirmation?: string) => Promise<void>;
  symbols: string[];
}

/**
 * The New trade form's state: the preview is re-read from Rust (the paper
 * fill at the live price) once typing settles; Open sends the same request.
 */
export function useNewTrade(status: BotDeskStatus, active = true): NewTradeController {
  const [form, setForm] = useState<TradeForm>(() => ({
    mode: "paper",
    kind: firstBot(status),
    symbol: "",
    direction: "long",
    sizing: "bot",
    notional: "",
    takeProfit: "",
    stopLoss: "",
  }));
  const [trade, setTrade] = useState<TradeState>({ state: "incomplete" });
  const [symbols, setSymbols] = useState<string[]>([]);
  const seq = useRef(0);
  const exchangeId = status[form.kind]?.exchangeId ?? "binance";

  useEffect(() => {
    if (!active) return;
    let alive = true;
    exchangeSymbols(exchangeId, form.kind === "futures")
      .then((s) => alive && setSymbols(s))
      .catch(() => alive && setSymbols([]));
    return () => {
      alive = false;
    };
  }, [active, exchangeId, form.kind]);

  useEffect(() => {
    const id = ++seq.current;
    const req = tradeRequest(form);
    if (req === "incomplete") {
      setTrade({ state: "incomplete" });
      return;
    }
    if (typeof req === "string") {
      setTrade({ state: "refused", code: req });
      return;
    }
    setTrade({ state: "loading" });
    const timer = window.setTimeout(() => {
      botPreviewTrade(req)
        .then((preview) => id === seq.current && setTrade({ state: "ready", preview, busy: false }))
        .catch((e: unknown) => id === seq.current && setTrade({ state: "refused", code: rawCode(e) }));
    }, PREVIEW_DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [form]);

  const set = (patch: Partial<TradeForm>) =>
    setForm((f) => {
      const next = { ...f, ...patch };
      // Real money: the Futures bot only.
      if (next.mode === "live") next.kind = "futures";
      // Spot is long only.
      if (next.kind === "spot") next.direction = "long";
      return next;
    });

  const open = async (confirmation?: string) => {
    if (trade.state !== "ready") return;
    const req = tradeRequest(form);
    if (typeof req === "string") return;
    if (req.mode === "live") {
      if (!confirmation) return;
      req.confirmation = confirmation;
      // The margin the confirmation showed: Rust re-plans at the open and
      // refuses (`liveTermsChanged`) when the money at risk moved.
      req.confirmedMarginUsdt = tradeTerms(trade.preview, "", 0).marginAtRiskUsdt;
    }
    const id = ++seq.current;
    setTrade({ ...trade, busy: true });
    try {
      const pos = await botOpenTrade(req);
      if (id === seq.current) setTrade({ state: "opened", symbol: pos.symbol, entry: pos.entry });
    } catch (e) {
      if (id === seq.current) setTrade({ state: "refused", code: rawCode(e) });
    }
  };

  return { form, set, trade, open, symbols };
}

/** Left panel: the trade's terms. */
export function NewTradeForm({ ctl, status }: { ctl: NewTradeController; status: BotDeskStatus }) {
  const { t } = useTranslation();
  const { form, set } = ctl;
  return (
    <form className="ae-newtrade__form" aria-label={t("positions.newTrade.title")} onSubmit={(e) => e.preventDefault()}>
      <div className="ae-newtrade__head">
        <h2 className="ae-section-title">{t("positions.newTrade.title")}</h2>
        {form.mode === "live" ? <LiveChip /> : <PaperChip />}
      </div>
      <div className="ae-field">
        <span className="ae-field__label">{t("liveMode.label")}</span>
        <ModePicker
          value={form.mode}
          onChange={(mode) => set({ mode })}
          blockers={liveTradeBlockers(status)}
          liveNote={t("liveMode.trade.liveNote")}
        />
      </div>
      <div className="ae-field">
        <span className="ae-field__label">{t("positions.newTrade.bot")}</span>
        <SegmentedControl
          label={t("positions.newTrade.bot")}
          value={form.kind}
          fill
          onChange={(kind) => set({ kind })}
          options={BOTS.map((k) => ({
            value: k,
            label: t(`bots.kind.${k}`),
            ...(status[k] === null ? { sub: t("positions.newTrade.notSaved") } : {}),
            disabled: form.mode === "live" && k !== "futures",
            reason: form.mode === "live" && k !== "futures" ? t("liveMode.blockers.futuresOnly") : undefined,
          }))}
        />
      </div>
      <TextField
        label={t("positions.newTrade.symbol")}
        className="mono"
        list="ae-newtrade-pairs"
        value={form.symbol}
        autoComplete="off"
        spellCheck={false}
        placeholder="BTCUSDT"
        onChange={(e) => set({ symbol: e.target.value.toUpperCase().replace(/[^A-Z0-9]/g, "") })}
      />
      <datalist id="ae-newtrade-pairs">
        {ctl.symbols.map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
      <div className="ae-field">
        <span className="ae-field__label">{t("positions.newTrade.side")}</span>
        <SegmentedControl
          label={t("positions.newTrade.side")}
          value={form.direction}
          fill
          onChange={(direction) => set({ direction })}
          options={(["long", "short"] as const).map((d) => ({
            value: d,
            label: t(`signalDesk.direction.${d}`),
            disabled: d === "short" && form.kind === "spot",
            reason: d === "short" && form.kind === "spot" ? t("bots.skipReasons.spotLongOnly") : undefined,
          }))}
        />
      </div>
      <div className="ae-field">
        <span className="ae-field__label">{t("positions.newTrade.size")}</span>
        <SegmentedControl
          label={t("positions.newTrade.size")}
          value={form.sizing}
          fill
          onChange={(sizing) => set({ sizing })}
          options={[
            { value: "bot", label: t("positions.newTrade.sizeBot") },
            { value: "usdt", label: "USDT" },
          ]}
        />
      </div>
      {form.sizing === "usdt" ? (
        <TextField
          label={t("positions.newTrade.notional")}
          inputMode="decimal"
          className="tabular"
          placeholder={String(EXCHANGE_MIN_ORDER_USDT)}
          hint={t("positions.newTrade.notionalMin", { min: EXCHANGE_MIN_ORDER_USDT })}
          value={form.notional}
          onChange={(e) => set({ notional: e.target.value })}
        />
      ) : null}
      <TextField
        label={t("positions.newTrade.takeProfit")}
        inputMode="decimal"
        className="tabular"
        value={form.takeProfit}
        onChange={(e) => set({ takeProfit: e.target.value })}
      />
      <TextField
        label={t("positions.newTrade.stopLoss")}
        inputMode="decimal"
        className="tabular"
        value={form.stopLoss}
        onChange={(e) => set({ stopLoss: e.target.value })}
      />
    </form>
  );
}

const RR_DIGITS = { minimumFractionDigits: 2, maximumFractionDigits: 2 };
const QTY_DIGITS = { maximumFractionDigits: 6 };

function Figures({ p }: { p: TradePreview }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const rows: { id: string; label: string; value: string; tone?: "up" | "down" }[] = [
    { id: "fill", label: t("positions.newTrade.fill"), value: formatPrice(p.entry, locale) },
    { id: "qty", label: t("positions.newTrade.qty"), value: formatNumber(p.qty, locale, QTY_DIGITS) },
    {
      id: "size",
      label: t("positions.newTrade.notionalFigure"),
      value: t("signalsPage.take.sizeValue", { amount: formatUsdt(p.notionalUsdt, locale, 2), leverage: p.effectiveLeverage.toFixed(2) }),
    },
    { id: "margin", label: t("positions.newTrade.margin"), value: formatUsdt(p.capital, locale, 2) },
    {
      id: "risk",
      label: t("positions.newTrade.lossAtStop", { price: formatPrice(p.stopLoss, locale) }),
      value: t("positions.newTrade.riskValue", {
        amount: formatSignedUsdt(p.lossAtStopUsdt, locale),
        pct: formatPercent(p.riskPctOfCapital, locale, 2),
      }),
      tone: "down",
    },
    {
      id: "gain",
      label: t("positions.newTrade.gainAtTarget", { price: formatPrice(p.takeProfit, locale) }),
      value: formatSignedUsdt(p.gainAtTargetUsdt, locale),
      tone: "up",
    },
    { id: "rr", label: t("positions.newTrade.rewardRisk"), value: formatNumber(p.rewardRisk, locale, RR_DIGITS) },
    ...(p.botKind === "spot"
      ? []
      : [
          {
            id: "liq",
            label: t("positions.newTrade.liquidation", { leverage: p.leverage }),
            value: p.liquidationPrice === null ? NO_VALUE : formatPrice(p.liquidationPrice, locale),
          },
        ]),
    { id: "fees", label: t("positions.newTrade.fees"), value: formatUsdt(p.feesUsdt, locale, 2) },
  ];
  return (
    <dl className="ae-newtrade__figures tabular">
      {rows.map((r) => (
        <div key={r.id} data-figure={r.id}>
          <dt>{r.label}</dt>
          <dd data-tone={r.tone}>{r.value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Center: the paper preview of the trade and the one button that opens it. */
export function NewTradePreview({ ctl, status, onClose }: { ctl: NewTradeController; status: BotDeskStatus; onClose: () => void }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { trade, form } = ctl;
  const [confirm, setConfirm] = useState(false);
  const live = form.mode === "live";
  const terms =
    live && trade.state === "ready" ? tradeTerms(trade.preview, status.futures?.exchangeId ?? "binance", status.pilotLeft ?? 0) : null;
  const title = t("positions.newTrade.previewTitle", {
    symbol: form.symbol.trim() || NO_VALUE,
    side: t(`signalDesk.direction.${form.direction}`),
    bot: t(`bots.kind.${form.kind}`),
  });
  return (
    <section className="ae-newtrade" aria-label={t("positions.newTrade.title")}>
      <div className="ae-newtrade__bar">
        <h2 className="ae-section-title">{title}</h2>
        {live ? <LiveChip /> : <PaperChip />}
        <span className="ae-toolbar__spacer" />
        <Button variant="ghost" size="xs" onClick={onClose}>
          {t("positions.newTrade.close")}
        </Button>
      </div>
      {trade.state === "incomplete" ? (
        <p className="ae-subtle">{t("positions.newTrade.incomplete")}</p>
      ) : trade.state === "loading" ? (
        <p className="ae-subtle">{t("signalsPage.take.pricing")}</p>
      ) : trade.state === "refused" ? (
        <p className="ae-newtrade__refusal" role="status">
          {tradeRefusalText(t, (k) => i18n.exists(k), trade.code)}
        </p>
      ) : trade.state === "opened" ? (
        <p className="ae-newtrade__opened" role="status">
          {t("positions.newTrade.opened", { symbol: trade.symbol, price: formatPrice(trade.entry, locale) })}
        </p>
      ) : (
        <>
          <Figures p={trade.preview} />
          <div className="ae-newtrade__actions">
            {trade.preview.riskCapped ? (
              <Chip tone="warn" title={t("bots.position.riskCappedTooltip")}>
                {t("bots.position.riskCappedBadge")}
              </Chip>
            ) : null}
            <span className="ae-subtle">{t("positions.newTrade.exitLine")}</span>
            <span className="ae-toolbar__spacer" />
            <Button variant={live ? "danger" : "primary"} size="sm" disabled={trade.busy} onClick={() => (live ? setConfirm(true) : void ctl.open())}>
              {trade.busy ? t("signalsPage.take.opening") : t(live ? "liveMode.trade.open" : "positions.newTrade.open")}
            </Button>
          </div>
          <PortfolioWhatIf candidate={tradeCandidate(trade.preview)} />
        </>
      )}
      <LiveConfirmDialog
        open={confirm && terms !== null}
        title={t("liveMode.trade.confirmTitle", { symbol: form.symbol.trim(), side: t(`signalDesk.direction.${form.direction}`) })}
        terms={terms}
        extra={
          trade.state === "ready"
            ? [{ label: t("positions.newTrade.lossAtStop", { price: formatPrice(trade.preview.stopLoss, locale) }), value: formatSignedUsdt(trade.preview.lossAtStopUsdt, locale), tone: "down" }]
            : []
        }
        note={t("liveMode.trade.note")}
        confirmLabel={t("liveMode.trade.open")}
        busy={trade.state === "ready" && trade.busy}
        onConfirm={(typed) => {
          setConfirm(false);
          void ctl.open(typed);
        }}
        onCancel={() => setConfirm(false)}
      />
    </section>
  );
}
