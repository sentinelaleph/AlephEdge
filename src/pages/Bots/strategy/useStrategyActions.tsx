import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { FactList } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPrice } from "@/lib/format";
import type { StrategyBotView } from "@/lib/ipc/strategy/strategy";
import type { StrategyAction } from "@/lib/ipc/strategy/useStrategyDesk";
import { neverStarted, sideText, strategyErrorText } from "@/lib/strategyText";
import { useStrategyLive } from "./useStrategyLive";

interface Pending {
  bot: StrategyBotView;
  action: StrategyAction;
}

export interface StrategyActions {
  /** Opens the confirmation for one action on one bot. */
  request: (bot: StrategyBotView, action: StrategyAction) => void;
  /** The dialog to render once in the page. */
  dialog: ReactNode;
  /** Translated failure of the last action, with the bot it was for. */
  error: { botId: string; text: string } | null;
  clearError: () => void;
}

/** Typed word per irreversible action; reversible ones need a click only. */
const WORD: Partial<Record<StrategyAction, string>> = { close: "CLOSE", archive: "DELETE" };

/**
 * Confirmations for Resume (strategy_start), Pause (strategy_stop), Close
 * cycle and stop (strategy_close) and Delete (strategy_archive). The dialog
 * says what changes, in labels and numbers, and whether real money moves: a
 * bot on real money mirrors every cycle on Binance (bot/strategy_live.rs).
 */
export function useStrategyActions(onDone?: (action: StrategyAction, bot: StrategyBotView) => void): StrategyActions {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { strategy } = useDeskContext();
  const [pending, setPending] = useState<Pending | null>(null);
  const live = useStrategyLive();
  const [error, setError] = useState<StrategyActions["error"]>(null);

  const usdt = (v: number) => `${formatNumber(v, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 })} USDT`;

  const confirm = async () => {
    if (!pending) return;
    const { bot, action } = pending;
    const err = await strategy.act(bot.id, action);
    setPending(null);
    if (err) setError({ botId: bot.id, text: strategyErrorText(t, err) });
    else {
      setError(null);
      onDone?.(action, bot);
    }
  };

  let dialog: ReactNode = null;
  if (pending) {
    const { bot, action } = pending;
    const c = bot.openCycle;
    const real = live.liveIds.has(bot.id);
    const firstStart = action === "start" && neverStarted(bot);
    const rows = [
      { label: t("table.symbol"), value: `${bot.symbol} · ${sideText(t, bot.side)}` },
      { label: t("strategy.field.budget"), value: usdt(bot.budget) },
      { label: t("table.mode"), value: real ? t("strategy.live.on") : t("states.paper") },
    ];
    if (c && (action === "close" || action === "stop" || action === "archive")) {
      rows.push(
        { label: t("strategy.detail.notional"), value: usdt(c.notional) },
        { label: t("strategy.detail.unrealized"), value: usdt(c.unrealizedQuote) },
      );
      if (bot.markPrice !== null) rows.push({ label: t("strategy.detail.markPrice"), value: formatPrice(bot.markPrice, locale) });
    }
    dialog = (
      <ConfirmDialog
        open
        title={t(`strategy.confirm.${firstStart ? "startFirst" : action}.title`, { name: bot.name })}
        body={
          <>
            <p>
              {t(
                action === "archive" && (c || bot.acceptingNewCycles)
                  ? "strategy.confirm.archive.bodyOpen"
                  : real && action !== "archive"
                    ? `strategy.confirm.${action}.bodyLive`
                    : `strategy.confirm.${action}.body`,
              )}
            </p>
            <FactList rows={rows} />
          </>
        }
        word={WORD[action]}
        confirmLabel={t(firstStart ? "strategy.actions.startFirst" : `strategy.actions.${action}`)}
        danger={action === "close" || action === "archive" || real}
        live={real}
        busy={strategy.busyId === bot.id}
        onConfirm={() => void confirm()}
        onCancel={() => setPending(null)}
      />
    );
  }

  return {
    request: (bot, action) => {
      setError(null);
      setPending({ bot, action });
    },
    dialog,
    error,
    clearError: () => setError(null),
  };
}
