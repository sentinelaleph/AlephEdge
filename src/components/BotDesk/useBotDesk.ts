import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  botClosePosition,
  botConfigure,
  botSetLive,
  botStart,
  botStatus,
  botStop,
  type BotConfig,
  type BotDeskStatus,
  type BotKind,
} from "@/lib/ipc/bot/bot";
import { errorMessage } from "@/lib/ipc/bridge";

const POLL_MS = 3000;

const EMPTY: BotDeskStatus = {
  liveTradingEnabled: false,
  binanceIsProduction: false,
  futures: null,
  spot: null,
  pump: null,
  futuresRunning: false,
  spotRunning: false,
  pumpRunning: false,
  openPositions: [],
  recentSkips: [],
  killSwitchTripped: false,
  // Before the first real status arrives the regime is honestly unknown —
  // matching the Rust side, which also starts Unknown.
  btcRegime: "unknown",
};

export interface BotDeskController {
  status: BotDeskStatus;
  /**
   * True once the first real `bot_status` has arrived. Until then `status` is
   * a placeholder: bot cards must not render from it, or a running bot's form
   * would show defaults and a Stop→Start would overwrite its real settings.
   */
  loaded: boolean;
  busy: boolean;
  error: string | null;
  configureAndStart: (config: BotConfig) => Promise<void>;
  stop: (kind: BotKind) => Promise<void>;
  /**
   * Switches real money on/off for one bot. Resolves to the rejection text
   * (shown next to the control that asked) or null on success; it never
   * throws and never touches the desk-wide `error`.
   */
  setLive: (kind: BotKind, enabled: boolean, confirmation: string) => Promise<string | null>;
  /**
   * Closes one open signal-bot position (exit reason "manual"). Resolves to
   * the localized rejection text, or null once the position has left the
   * book; never throws and never touches the desk-wide `error`.
   */
  closePosition: (signalId: string, kind: BotKind) => Promise<string | null>;
}

/** Polls the bot desk and exposes configure/start/stop actions. */
export function useBotDesk(): BotDeskController {
  const { t } = useTranslation();
  const [status, setStatus] = useState<BotDeskStatus>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const alive = useRef(true);

  useEffect(() => {
    alive.current = true;
    let timer: number | undefined;
    const tick = async () => {
      try {
        const s = await botStatus();
        if (alive.current) {
          setStatus(s);
          setLoaded(true);
        }
      } catch {
        /* keep last-good on transient error */
      }
      if (alive.current) timer = window.setTimeout(tick, POLL_MS);
    };
    void tick();
    return () => {
      alive.current = false;
      if (timer) window.clearTimeout(timer);
    };
  }, []);

  const run = useCallback(async (action: () => Promise<BotDeskStatus>) => {
    setError(null);
    setBusy(true);
    try {
      setStatus(await action());
    } catch (e) {
      setError(errorMessage(e, t("bots.actionFailed")));
    } finally {
      setBusy(false);
    }
  }, [t]);

  // Configure THEN start in one awaited chain — `bot_start` rejects with
  // "bot not configured" if it lands before the config is stored, so the two
  // must never race (they used to be fired independently).
  const configureAndStart = useCallback(
    (config: BotConfig) =>
      run(async () => {
        await botConfigure(config);
        return botStart(config.kind);
      }),
    [run],
  );
  const stop = useCallback((kind: BotKind) => run(() => botStop(kind)), [run]);

  const setLive = useCallback(
    async (kind: BotKind, enabled: boolean, confirmation: string) => {
      setBusy(true);
      try {
        const s = await botSetLive(kind, enabled, confirmation);
        if (alive.current) setStatus(s);
        return null;
      } catch (e) {
        return errorMessage(e, t("bots.live.failed"));
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [t],
  );

  const closePosition = useCallback(
    async (signalId: string, kind: BotKind) => {
      try {
        const s = await botClosePosition(signalId, kind);
        if (alive.current) setStatus(s);
        return null;
      } catch (e) {
        // Whatever happened, the book may have moved: re-read it.
        botStatus()
          .then((s) => alive.current && setStatus(s))
          .catch(() => undefined);
        return errorMessage(e, t("positions.closeFailed"));
      }
    },
    [t],
  );

  return { status, loaded, busy, error, configureAndStart, stop, setLive, closePosition };
}
