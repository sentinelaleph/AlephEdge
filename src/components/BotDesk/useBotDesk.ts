import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  botCloseLiveAndPaper,
  botClosePosition,
  botConfigure,
  botEndPilot,
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
  /** Saves a config without starting; the rejection text or null. Never throws. */
  configure: (config: BotConfig) => Promise<string | null>;
  /** Starts a configured bot; the rejection text or null. Never throws. */
  start: (kind: BotKind) => Promise<string | null>;
  stop: (kind: BotKind) => Promise<void>;
  /** Reads the desk now (after an action elsewhere, e.g. "Stop all live"). */
  refresh: () => Promise<void>;
  /**
   * Real money for one bot: "Go live", the per-session arm, or off.
   * `atRiskUsdt` is the margin the confirmation showed (Rust re-checks it).
   * Resolves to the rejection text (shown next to the control that asked) or
   * null on success; it never throws and never touches the desk-wide `error`.
   */
  setLive: (kind: BotKind, enabled: boolean, confirmation: string, atRiskUsdt?: number) => Promise<string | null>;
  /** Ends a live bot's pilot (full-size entries again); the rejection text or null. */
  endPilot: (kind: BotKind) => Promise<string | null>;
  /**
   * Closes one open signal-bot position (exit reason "manual"). Resolves to
   * the localized rejection text, or null once the position has left the
   * book; never throws and never touches the desk-wide `error`.
   */
  closePosition: (signalId: string, kind: BotKind) => Promise<string | null>;
  /**
   * "Close positions and switch": closes the Live bot's own real positions
   * (reduce-only market orders), then makes it Paper. The rejection text
   * (some still open: the bot stays a disarmed Live bot) or null.
   */
  closeLiveAndPaper: (kind: BotKind) => Promise<string | null>;
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
  // must never race.
  const configureAndStart = useCallback(
    (config: BotConfig) =>
      run(async () => {
        await botConfigure(config);
        return botStart(config.kind);
      }),
    [run],
  );
  const stop = useCallback((kind: BotKind) => run(() => botStop(kind)), [run]);

  /** One awaited call whose rejection is returned as text, not put in `error`. */
  const attempt = useCallback(
    async (action: () => Promise<BotDeskStatus>, fallback: string) => {
      setBusy(true);
      try {
        const s = await action();
        if (alive.current) setStatus(s);
        return null;
      } catch (e) {
        return errorMessage(e, fallback);
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [],
  );
  const configure = useCallback((config: BotConfig) => attempt(() => botConfigure(config), t("bots.actionFailed")), [attempt, t]);
  const start = useCallback((kind: BotKind) => attempt(() => botStart(kind), t("bots.actionFailed")), [attempt, t]);
  const refresh = useCallback(async () => {
    try {
      const s = await botStatus();
      if (alive.current) setStatus(s);
    } catch {
      /* the poll retries */
    }
  }, []);

  const setLive = useCallback(
    async (kind: BotKind, enabled: boolean, confirmation: string, atRiskUsdt?: number) => {
      setBusy(true);
      try {
        const s = await botSetLive(kind, enabled, confirmation, atRiskUsdt);
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

  const endPilot = useCallback(
    async (kind: BotKind) => {
      try {
        const s = await botEndPilot(kind);
        if (alive.current) setStatus(s);
        return null;
      } catch (e) {
        return errorMessage(e, t("bots.live.failed"));
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

  const closeLiveAndPaper = useCallback(
    async (kind: BotKind) => {
      setBusy(true);
      try {
        const s = await botCloseLiveAndPaper(kind);
        if (alive.current) setStatus(s);
        return null;
      } catch (e) {
        botStatus()
          .then((s) => alive.current && setStatus(s))
          .catch(() => undefined);
        return errorMessage(e, t("positions.closeFailed"));
      } finally {
        if (alive.current) setBusy(false);
      }
    },
    [t],
  );

  return { status, loaded, busy, error, configureAndStart, configure, start, stop, refresh, setLive, endPilot, closePosition, closeLiveAndPaper };
}
