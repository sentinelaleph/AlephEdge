import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile } from "@/components/ui/KpiTile/KpiTile";
import { localeForLanguage } from "@/i18n";
import { formatPnl, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { isActive, type StrategyKind } from "@/lib/ipc/strategy/strategy";
import { allBotRows, signalBotRows } from "./botRows";
import { strategyBotMoney, sumMoney, totalOf, useSignalBotMoney } from "./botMoney";

export type BotsScope = "all" | "signal" | StrategyKind;

/**
 * The bots of a page at a glance: how many run, what they have open, what
 * that would book now, what they have realized, and the two together. The
 * same figures as the bot tables below, summed.
 */
export function BotsSummary({ scope }: { scope: BotsScope }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk, strategy } = useDeskContext();
  const signalMoney = useSignalBotMoney();
  if (!desk.loaded) return null;

  // All bots counts the TradingView bot too; the signal page its own three.
  const cap = risk.state?.maxCapitalQuote ?? null;
  const book = scope === "all" ? allBotRows(desk.status, cap) : scope === "signal" ? signalBotRows(desk.status, cap) : [];
  const signals = book.filter((r) => r.config);
  const strategies = scope === "signal" ? [] : (strategy.bots ?? []).filter((b) => scope === "all" || b.kind === scope);
  const total = signals.length + strategies.length;
  if (total === 0) return null;
  const running = signals.filter((r) => r.running).length + strategies.filter(isActive).length;
  const m = sumMoney([...signals.map((r) => signalMoney(r.kind)), ...strategies.map(strategyBotMoney)]);
  const positions = signals.reduce((n, r) => n + r.openPositions, 0);
  const cycles = strategies.filter((b) => b.openCycle).length;
  const all = totalOf(m);
  const usdt = (n: number | null) => (n === null ? NO_VALUE : formatPnl(n, locale).text);

  return (
    <KpiGrid minTile={150} label={t("botsSummary.label")}>
      <KpiTile label={t("botsSummary.running")} value={t("botsSummary.runningOf", { running, total })} />
      <KpiTile
        label={t("dashboard.col.openPnl")}
        value={m.open === 0 ? NO_VALUE : usdt(m.openPnl)}
        tone={m.open === 0 || m.openPnl === null ? undefined : pnlToneAttr(m.openPnl)}
        note={
          scope === "signal"
            ? t("botsSummary.positions", { count: positions })
            : scope === "all"
              ? t("botsSummary.positionsCycles", { positions, cycles })
              : t("botsSummary.cycles", { count: cycles })
        }
      />
      <KpiTile label={t("dashboard.col.realized")} value={usdt(m.realized)} tone={pnlToneAttr(m.realized)} note={t("botsSummary.realizedNote")} />
      <KpiTile label={t("dashboard.col.total")} value={usdt(all)} tone={pnlToneAttr(all)} note={t("botsSummary.totalNote")} />
    </KpiGrid>
  );
}
