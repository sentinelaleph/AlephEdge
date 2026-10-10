import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { KEYS_PATH } from "@/app/router/routes";
import { Link } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, formatPrice, formatTableTime, NO_VALUE } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  parseStrategyError,
  STRATEGY_LIVE_WORD,
  strategyEndPilot,
  strategySetLive,
  type StrategyBotView,
  type StrategyConfig,
} from "@/lib/ipc/strategy/strategy";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { strategyErrorText } from "@/lib/strategyText";
import { liveTargetKey, liveVenueAllowed, tradeVenues } from "@/lib/venues";
import { isLiveView, useStrategyLive } from "./useStrategyLive";

const EXCHANGE_PATH = "/positions?tab=exchange";
const GUIDE_PATH = "/guide?s=real-money";
const PREFLIGHT_PATH = "/risk?s=preflight";

/** Whether the config has a level the exchange stop can rest at. */
function hasProtectiveLevel(cfg: StrategyConfig): boolean {
  if ((cfg.maxDrawdownPct ?? 0) > 0) return true;
  return cfg.params.kind === "dca" ? (cfg.params.slPct ?? 0) > 0 : (cfg.params.stopOutPct ?? 0) > 0;
}

/** "BTCUSDT" → "BTC": the unit of a position quantity. */
function baseAsset(symbol: string): string {
  return symbol.replace(/(USDT|USDC|BUSD|FDUSD)$/, "");
}

/**
 * Paper or real money for one DCA / Grid bot. Real money mirrors the
 * simulated position on Binance futures with market orders and keeps a stop
 * on the exchange (bot/strategy_live.rs). Before switching, the panel lists
 * what is still missing, each with the place that fixes it.
 */
export function StrategyLivePanel({ view, config }: { view: StrategyBotView; config: StrategyConfig | null }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, vault, catalog } = useDeskContext();
  const { viewFor, setViews } = useStrategyLive();
  const [dialog, setDialog] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ code: string; text: string } | null>(null);
  const live = viewFor(view.id);
  const enabled = live?.enabled === true;
  const real = isLiveView(live);
  const buildOn = desk.loaded && desk.status.liveTradingEnabled;
  const futures = view.market === "futures" && view.exchangeId === "binance";
  const settingsPath = `/bots/${view.id}?tab=settings`;

  // Venues with a verified trade-only key; Binance first when present.
  const venues = tradeVenues(vault.credentials);
  // Real money switches on only where the order path passed a test-network
  // dry run (Rust refuses the rest: liveVenueNotDryRun).
  const liveVenues = desk.status.liveVenues;
  const [venuePick, setVenuePick] = useState<string | null>(null);
  const venue = live?.enabled
    ? live.venue
    : (venuePick ?? venues.find((v) => liveVenueAllowed(liveVenues, v)) ?? venues[0] ?? "binance");
  const venueName = (id: string) => exchangeName(id, catalog.exchanges);
  const keyOk = venues.includes(venue);
  const venueOk = liveVenueAllowed(liveVenues, venue);
  const stopOk = config ? hasProtectiveLevel(config) : false;
  const idleOk = view.openCycle === null;
  const requirements = [
    { ok: keyOk, label: t("strategy.live.req.key"), to: KEYS_PATH, go: t("strategy.live.goKeys") },
    { ok: stopOk, label: t("strategy.live.req.stop"), to: settingsPath, go: t("strategy.live.goSettings") },
    { ok: idleOk, label: t("strategy.live.req.idle"), to: null, go: "" },
  ];
  const ready = venueOk && requirements.every((r) => r.ok);

  /** Where an error is fixed. */
  const fixFor = (code: string): { to: string; label: string } | null => {
    if (code === "liveNeedsVerifiedKey" || code === "liveNeedsTradeKey") return { to: KEYS_PATH, label: t("strategy.live.goKeys") };
    if (code === "liveNeedsStop" || code === "liveOrderBelowMinNotional") return { to: settingsPath, label: t("strategy.live.goSettings") };
    if (code === "liveCloseFirst" || code === "liveSymbolBusy") return { to: EXCHANGE_PATH, label: t("strategy.live.goExchange") };
    return null;
  };

  /** `confirmation` is what the user typed (Rust re-checks it is LIVE). */
  const set = async (on: boolean, confirmation = "") => {
    setBusy(true);
    setError(null);
    try {
      setViews(await strategySetLive(view.id, on, on ? confirmation : "", on ? venue : undefined));
    } catch (e) {
      const raw = errorMessage(e, "liveExchangeCheckFailed");
      setError({ code: parseStrategyError(raw).code, text: strategyErrorText(t, raw) });
    } finally {
      setBusy(false);
      setDialog(false);
    }
  };

  const chip = live?.halted ? (
    <StatusChip status="error" label={t("strategy.live.halted")} />
  ) : enabled ? (
    <LiveChip />
  ) : (
    <StatusChip status="ok" label={t("strategy.live.paper")} />
  );

  const usd = (v: number) => formatPnl(v, locale, { unit: "none" }).text;
  const rows =
    real || (live && live.fills > 0)
      ? [
          { label: t("strategy.live.venue"), value: live ? venueName(live.venue) : NO_VALUE },
          {
            label: t("strategy.live.realQty"),
            value: live && live.realQty !== 0 ? `${live.realQty} ${baseAsset(view.symbol)}` : NO_VALUE,
          },
          { label: t("strategy.live.entry"), value: live && live.entryPrice > 0 ? formatPrice(live.entryPrice, locale) : NO_VALUE },
          { label: t("strategy.live.unrealized"), value: live && live.realQty !== 0 ? `${usd(live.unrealizedUsdt)} USDT` : NO_VALUE },
          { label: t("strategy.live.stop"), value: live?.stopPrice ? formatPrice(live.stopPrice, locale) : NO_VALUE },
          { label: t("strategy.live.realized"), value: live ? `${usd(live.realizedGrossUsdt)} USDT` : NO_VALUE },
          { label: t("strategy.live.fees"), value: live ? `${usd(-live.feesEstUsdt)} USDT` : NO_VALUE },
          { label: t("strategy.live.fills"), value: live ? String(live.fills) : NO_VALUE },
          { label: t("strategy.live.lastSync"), value: live && live.lastSyncMs > 0 ? formatTableTime(live.lastSyncMs, locale) : NO_VALUE },
        ]
      : [];
  const fix = error ? fixFor(error.code) : null;
  const pilot = enabled && live && live.pilotCyclesLeft > 0;
  const endPilot = async () => {
    setBusy(true);
    try {
      setViews(await strategyEndPilot(view.id));
    } catch (e) {
      const raw = errorMessage(e, "liveExchangeCheckFailed");
      setError({ code: parseStrategyError(raw).code, text: strategyErrorText(t, raw) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Panel title={t("strategy.live.title")} aside={chip}>
      {rows.length ? <FactList rows={rows} /> : null}
      {pilot && live ? (
        <p className="ae-muted">
          {t("strategy.live.pilot", {
            count: live.pilotCyclesLeft,
            pct: Math.round(live.cycleFactor * 100),
          })}{" "}
          <Button variant="ghost" size="xs" disabled={busy} onClick={() => void endPilot()}>
            {t("strategy.live.endPilot")}
          </Button>
        </p>
      ) : null}
      {live?.halted ? (
        <>
          <p className="ae-error">{t(`strategy.notes.${live.halted}`, { defaultValue: live.halted })}</p>
          <p>
            <Link to={EXCHANGE_PATH} className="ae-link">
              {t("strategy.live.goExchange")}
            </Link>
          </p>
        </>
      ) : null}
      {!buildOn ? <p className="ae-muted">{t("strategy.live.buildOff")}</p> : null}
      {buildOn && !futures ? <p className="ae-muted">{t("strategy.live.futuresOnly")}</p> : null}
      {buildOn && futures && !enabled && venues.length > 1 ? (
        <label className="ae-field">
          <span className="ae-field__label">{t("strategy.live.venue")}</span>
          <select className="ae-field__input" value={venue} onChange={(e) => setVenuePick(e.target.value)} disabled={busy}>
            {venues.map((v) => (
              <option key={v} value={v}>
                {venueName(v)}
              </option>
            ))}
          </select>
        </label>
      ) : null}
      {buildOn && futures && !enabled && !venueOk ? (
        <p className="ae-error" role="note">
          {t("vault.venueNotDryRun", { exchange: venueName(venue) })}
        </p>
      ) : null}
      {buildOn && futures && !enabled ? (
        <ul className="ae-list" aria-label={t("strategy.live.req.title")}>
          {requirements.map((r) => (
            <li key={r.label} className={r.ok ? "ae-muted" : undefined}>
              <span aria-hidden="true">{r.ok ? "✓ " : "✗ "}</span>
              {r.label}
              {!r.ok && r.to ? (
                <>
                  {" · "}
                  <Link to={r.to} className="ae-link">
                    {r.go}
                  </Link>
                </>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {error ? (
        <p className="ae-error" role="alert">
          {error.text}
          {fix ? (
            <>
              {" · "}
              <Link to={fix.to} className="ae-link">
                {fix.label}
              </Link>
            </>
          ) : null}
        </p>
      ) : null}
      {buildOn && futures ? (
        <div className="ae-toolbar">
          {enabled ? (
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void set(false)}>
              {t("strategy.live.disable")}
            </Button>
          ) : (
            <Button variant="danger" size="sm" disabled={busy || !ready} onClick={() => setDialog(true)}>
              {t("strategy.live.enable")}
            </Button>
          )}
          <Link to={PREFLIGHT_PATH} className="ae-link">
            {t("preflight.title")}
          </Link>
          <Link to={GUIDE_PATH} className="ae-link">
            {t("strategy.live.goGuide")}
          </Link>
        </div>
      ) : null}
      <ConfirmDialog
        open={dialog}
        live
        danger
        title={t("strategy.live.confirmTitle", { name: view.name })}
        body={
          <>
            <p>{t(liveTargetKey(venue, desk.status.binanceIsProduction, desk.status.venueSandbox), { exchange: venueName(venue) })}</p>
            <p>
              {t("strategy.live.confirmBudget", {
                budget: formatNumber(view.budget, locale),
                leverage: formatNumber(view.leverage, locale),
              })}
            </p>
            <p>{t("strategy.live.confirmBodyVenue", { symbol: view.symbol, leverage: view.leverage, exchange: venueName(venue) })}</p>
          </>
        }
        word={STRATEGY_LIVE_WORD}
        confirmLabel={t("strategy.live.enable")}
        busy={busy}
        onConfirm={(typed) => void set(true, typed)}
        onCancel={() => setDialog(false)}
      />
    </Panel>
  );
}
