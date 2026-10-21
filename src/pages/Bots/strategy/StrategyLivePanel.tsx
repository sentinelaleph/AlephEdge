import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { KEYS_PATH } from "@/app/router/routes";
import { Link } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip } from "@/components/ui/Chip/Chip";
import { LiveConfirmDialog } from "@/components/LiveMode/LiveMode";
import type { LiveTerms } from "@/lib/ipc/bot/bot";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { formatPnl, formatPrice, formatTableTime, NO_VALUE } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import {
  parseStrategyError,
  strategyArmLive,
  strategyEndPilot,
  strategyLiveTerms,
  strategySetLive,
  type StrategyBotView,
  type StrategyConfig,
} from "@/lib/ipc/strategy/strategy";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { strategyErrorText } from "@/lib/strategyText";
import { hasProtectiveLevel } from "@/lib/liveMode";
import { liveVenueAllowed, tradeVenues } from "@/lib/venues";
import { isLiveView, useStrategyLive } from "./useStrategyLive";

const EXCHANGE_PATH = "/positions?tab=exchange";
const GUIDE_PATH = "/guide?s=real-money";
const PREFLIGHT_PATH = "/risk?s=preflight";

/** "BTCUSDT" → "BTC": the unit of a position quantity. */
function baseAsset(symbol: string): string {
  return symbol.replace(/(USDT|USDC|BUSD|FDUSD)$/, "");
}

/**
 * Paper or Live for one DCA / Grid bot. Live mirrors the simulated position
 * on the exchange's futures with market orders and keeps a stop there
 * (bot/strategy_live.rs). "Go live" on a Paper bot and the per-session arm
 * of a Live bot both show the money at risk and need the typed LIVE. Before
 * going live, the panel lists what is still missing, each with its fix.
 */
export function StrategyLivePanel({ view, config }: { view: StrategyBotView; config: StrategyConfig | null }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, vault, catalog } = useDeskContext();
  const { viewFor, setViews } = useStrategyLive();
  /** The confirmation open: going live, or arming a Live bot, with its terms. */
  const [dialog, setDialog] = useState<{ arm: boolean; terms: LiveTerms } | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<{ code: string; text: string } | null>(null);
  const live = viewFor(view.id);
  const enabled = live?.enabled === true;
  const real = isLiveView(live);
  const armed = enabled && live?.armed === true;
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

  const fail = (e: unknown) => {
    const raw = errorMessage(e, "liveExchangeCheckFailed");
    setError({ code: parseStrategyError(raw).code, text: strategyErrorText(t, raw) });
  };

  /** Opens the confirmation with the money at risk read from Rust. */
  const ask = async (arm: boolean) => {
    if (!config) return;
    setError(null);
    try {
      setDialog({ arm, terms: await strategyLiveTerms(config, arm && live ? live.venue : venue) });
    } catch (e) {
      fail(e);
    }
  };

  /** `typed` is what the user typed (Rust re-checks it is LIVE, and the margin). */
  const confirm = async (typed: string) => {
    if (!dialog) return;
    setBusy(true);
    setError(null);
    try {
      const margin = dialog.terms.marginAtRiskUsdt;
      setViews(dialog.arm ? await strategyArmLive(view.id, typed, margin) : await strategySetLive(view.id, true, typed, venue, margin));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
      setDialog(null);
    }
  };

  const off = async () => {
    setBusy(true);
    setError(null);
    try {
      setViews(await strategySetLive(view.id, false, ""));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  };

  const chip = live?.halted ? (
    <StatusChip status="error" label={t("strategy.live.halted")} />
  ) : enabled ? (
    <span className="ae-toolbar">
      <LiveChip />
      {!armed ? <span className="ae-subtle">{t("liveMode.header.needsArm")}</span> : null}
    </span>
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
      {enabled && !armed && !live?.halted ? <p className="ae-muted">{t("liveMode.strategy.needsArm")}</p> : null}
      {!futures ? <p className="ae-muted">{t("strategy.live.futuresOnly")}</p> : null}
      {futures && !enabled && venues.length > 1 ? (
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
      {futures && !enabled && !venueOk ? (
        <p className="ae-error" role="note">
          {t("vault.venueNotDryRun", { exchange: venueName(venue) })}
        </p>
      ) : null}
      {futures && !enabled ? (
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
      {futures ? (
        <div className="ae-toolbar">
          {enabled && !armed && !live?.halted ? (
            <Button variant="danger" size="sm" disabled={busy || !config} onClick={() => void ask(true)}>
              {t("liveMode.strategy.arm")}
            </Button>
          ) : null}
          {enabled ? (
            <Button variant="secondary" size="sm" disabled={busy} onClick={() => void off()}>
              {t("strategy.live.disable")}
            </Button>
          ) : (
            <Button variant="danger" size="sm" disabled={busy || !ready || !config} onClick={() => void ask(false)}>
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
      <LiveConfirmDialog
        open={dialog !== null}
        title={t(dialog?.arm ? "liveMode.strategy.armTitle" : "strategy.live.confirmTitle", { name: view.name })}
        terms={dialog?.terms ?? null}
        note={
          dialog?.arm
            ? t("liveMode.restartNote")
            : t("strategy.live.confirmBodyVenue", { symbol: view.symbol, leverage: view.leverage, exchange: venueName(venue) })
        }
        confirmLabel={t(dialog?.arm ? "liveMode.strategy.arm" : "strategy.live.enable")}
        busy={busy}
        onConfirm={(typed) => void confirm(typed)}
        onCancel={() => setDialog(null)}
      />
    </Panel>
  );
}
