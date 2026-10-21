import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { LiveConfirmDialog } from "@/components/LiveMode/LiveMode";
import { errorMessage } from "@/lib/ipc/bridge";
import { botLiveTerms, botMode, type BotConfig, type BotKind, type LiveTerms } from "@/lib/ipc/bot/bot";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { holdsPaper, liveBlockers, liveWordKey, ordersOnTestnet } from "@/lib/liveMode";
import { liveVenueAllowed, ORDER_VENUES, tradeVenues } from "@/lib/venues";
import type { BotLiveControl } from "./SignalBotForm";

/**
 * Paper / Live for one signal bot: the form's wiring and the typed-LIVE
 * dialog. Start with Live chosen saves the settings, reads the money at risk
 * from Rust (`bot_live_terms`), asks for LIVE, then arms (`bot_set_live`,
 * which re-checks that margin) and starts.
 */
export function useSignalLive(kind: BotKind): { control: BotLiveControl; dialog: ReactNode } {
  const { t } = useTranslation();
  const { desk, vault, catalog } = useDeskContext();
  const s = desk.status;
  const cfg = s[kind];
  const [terms, setTerms] = useState<LiveTerms | null>(null);
  const [error, setError] = useState<string | null>(null);
  const venue = cfg?.exchangeId ?? "binance";
  const venueName = exchangeName(venue, catalog.exchanges);
  const extra = holdsPaper(s.openPositions, kind) ? [{ key: "liveMode.blockers.paperPositions" }] : [];
  const blockers = liveBlockers({
    // The Futures bot and the TradingView bot trade USDT-M futures.
    futures: (kind === "futures" || kind === "tradingview") && (ORDER_VENUES as readonly string[]).includes(venue),
    keyOk: tradeVenues(vault.credentials).includes(venue),
    venueOk: liveVenueAllowed(s.liveVenues, venue),
    venueName,
    tradingBlocked: s.tradingBlocked ?? null,
    extra,
  });

  const onStartLive = async (config: BotConfig) => {
    setError(null);
    const saved = await desk.configure(config);
    if (saved) return setError(saved);
    try {
      setTerms(await botLiveTerms(kind));
    } catch (e) {
      setError(errorMessage(e, t("bots.live.failed")));
    }
  };

  const confirm = async (typed: string) => {
    if (!terms) return;
    const armed = await desk.setLive(kind, true, typed, terms.marginAtRiskUsdt);
    setTerms(null);
    if (armed) return setError(armed);
    const started = await desk.start(kind);
    if (started) setError(started);
  };

  const testnet = ordersOnTestnet(s, desk.loaded);
  const control: BotLiveControl = {
    pilotLeft: (kind === "tradingview" ? s.tradingviewPilotLeft : s.pilotLeft) ?? 0,
    onEndPilot: () => desk.endPilot(kind),
    blockers,
    onStartLive: (c) => void onStartLive(c),
    onToPaper: async () => {
      setError(null);
      return desk.setLive(kind, false, "");
    },
    realPositions: s.openPositions.filter((p) => p.botKind === kind && p.live),
    onCloseAndPaper: async () => {
      setError(null);
      return desk.closeLiveAndPaper(kind);
    },
    testnet,
    error,
  };

  const live = botMode(cfg) === "live";
  const dialog = (
    <LiveConfirmDialog
      open={terms !== null}
      title={t(live ? "liveMode.signal.armTitle" : liveWordKey("goLiveTitle", testnet), { bot: t(`bots.kind.${kind}`) })}
      terms={terms}
      note={t("liveMode.restartNote")}
      confirmLabel={t(liveWordKey("startLive", testnet))}
      busy={desk.busy}
      onConfirm={(typed) => void confirm(typed)}
      onCancel={() => setTerms(null)}
    />
  );
  return { control, dialog };
}
