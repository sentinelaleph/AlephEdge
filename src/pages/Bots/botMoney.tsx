import { useMemo } from "react";
import type { TFunction } from "i18next";
import { useDeskContext } from "@/app/DeskProvider";
import type { DataColumn } from "@/components/ui/DataTable/DataTable";
import { formatPnl, NO_VALUE, pnlToneAttr } from "@/lib/format";
import type { BotKind } from "@/lib/ipc/bot/bot";
import type { StrategyBotView } from "@/lib/ipc/strategy/strategy";
import { marksByPosition, openPnl } from "@/lib/positionMarks";

/** Where one bot stands: what is open, what it would book now, what it has booked. */
export interface BotMoney {
  /** Open positions (signal bot) or 1 for an open cycle (DCA / Grid). */
  open: number;
  /** P&L if everything open closed at the last price; null until every open position is priced. */
  openPnl: number | null;
  /** Closed trades or cycles after fees, all time; null while loading. */
  realized: number | null;
}

/** Realized plus open; null when either part is not known yet. */
export function totalOf(m: BotMoney): number | null {
  if (m.realized === null || (m.open > 0 && m.openPnl === null)) return null;
  return m.realized + (m.open > 0 ? (m.openPnl ?? 0) : 0);
}

/** Sums the money of several bots; null parts stay null (never half a figure). */
export function sumMoney(rows: BotMoney[]): BotMoney {
  return rows.reduce<BotMoney>(
    (acc, m) => ({
      open: acc.open + m.open,
      openPnl: acc.openPnl === null || (m.open > 0 && m.openPnl === null) ? null : acc.openPnl + (m.open > 0 ? (m.openPnl ?? 0) : 0),
      realized: acc.realized === null || m.realized === null ? null : acc.realized + m.realized,
    }),
    { open: 0, openPnl: 0, realized: 0 },
  );
}

/** Money of a signal bot, from the engine's marks and the per-bot trade stats. */
export function useSignalBotMoney(): (kind: BotKind) => BotMoney {
  const { desk, pnl } = useDeskContext();
  const marks = useMemo(() => marksByPosition(desk.status), [desk.status]);
  const stats = useMemo(() => new Map((pnl.byBot ?? []).map((b) => [b.botKind, b.stats])), [pnl.byBot]);
  return (kind) => {
    const o = openPnl(desk.status.openPositions.filter((p) => p.botKind === kind), marks);
    return {
      open: o.open,
      openPnl: o.priced === o.open ? o.pnlUsdt : null,
      realized: pnl.byBot === null ? null : (stats.get(kind)?.netPnlQuote ?? 0),
    };
  };
}

/** Money of a DCA / Grid bot, from its own view. */
export function strategyBotMoney(b: StrategyBotView): BotMoney {
  return { open: b.openCycle ? 1 : 0, openPnl: b.openCycle?.unrealizedQuote ?? 0, realized: b.realizedQuote };
}

/** Open P&L, Realized and Total columns for any bot table. */
export function botMoneyColumns<R>(t: TFunction, locale: string, money: (row: R) => BotMoney | null): DataColumn<R>[] {
  const signed = (v: number | null) => (v === null ? NO_VALUE : formatPnl(v, locale, { unit: "none" }).text);
  return [
    {
      id: "openPnl",
      header: t("dashboard.col.openPnl"),
      headerTip: t("dashboard.col.openPnlTip"),
      numeric: true,
      cell: (r) => {
        const m = money(r);
        return !m || m.open === 0 ? NO_VALUE : signed(m.openPnl);
      },
      tone: (r) => {
        const m = money(r);
        return !m || m.open === 0 || m.openPnl === null ? "muted" : pnlToneAttr(m.openPnl);
      },
    },
    {
      id: "realized",
      header: t("dashboard.col.realized"),
      headerTip: t("dashboard.col.realizedTip"),
      numeric: true,
      priority: 2,
      cell: (r) => signed(money(r)?.realized ?? null),
      tone: (r) => pnlToneAttr(money(r)?.realized ?? null),
    },
    {
      id: "total",
      header: t("dashboard.col.total"),
      headerTip: t("dashboard.col.totalTip"),
      numeric: true,
      keep: true,
      cell: (r) => {
        const m = money(r);
        return signed(m ? totalOf(m) : null);
      },
      tone: (r) => {
        const m = money(r);
        const total = m ? totalOf(m) : null;
        return total === null ? "muted" : pnlToneAttr(total);
      },
    },
  ];
}
