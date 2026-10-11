import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile, Meter } from "@/components/ui/KpiTile/KpiTile";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, formatUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { dailyRoom, stopRisk, stopsWouldTrip, strategyRoom } from "@/lib/riskReadout";

/**
 * Risk & safety, at the top: what every stop filling would book now, the
 * room left before the daily stop, capital in use, and the DCA / Grid budget
 * and breaker. Each figure is read from the desk; none is estimated.
 */
export function RiskSummary() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk, pnl, strategy } = useDeskContext();
  const s = desk.status;
  const r = risk.state;
  const usdt = (n: number) => formatUsdt(n, locale);
  const signed = (n: number) => formatPnl(n, locale).text;
  const pct = (part: number, whole: number) => formatNumber((part / whole) * 100, locale, { maximumFractionDigits: 1 });

  const stops = stopRisk(s.openPositions, s.atStop);
  const today = pnl.stats?.todayPnlQuote ?? null;
  const room = r && today !== null ? dailyRoom(r.balance, r.effectiveDailyLossPct, today) : null;
  const tripped = desk.loaded && s.killSwitchTripped;
  const wouldTrip = room ? stopsWouldTrip(stops.atStops, room) : false;
  const sr = strategy.risk ? strategyRoom(strategy.risk) : null;

  return (
    <KpiGrid minTile={170} label={t("safety.summary.label")}>
      <KpiTile
        label={t("safety.summary.atStops")}
        value={!desk.loaded ? null : stops.positions === 0 ? NO_VALUE : signed(stops.atStops)}
        tone={stops.positions === 0 ? undefined : pnlToneAttr(stops.atStops)}
        note={
          !desk.loaded
            ? undefined
            : stops.positions === 0
              ? t("safety.summary.noPositions")
              : r
                ? t("safety.summary.atStopsNote", { count: stops.positions, pct: pct(-Math.min(0, stops.atStops), r.balance) })
                : undefined
        }
      />
      <KpiTile
        label={t("safety.summary.dailyLeft")}
        value={room === null ? null : tripped ? NO_VALUE : usdt(room.left)}
        tone={tripped || wouldTrip ? "down" : undefined}
        note={
          room === null ? undefined : (
            <>
              <Meter fraction={room.limit > 0 ? room.used / room.limit : 0} label={t("safety.killSwitch.meter")} />
              <span>
                {tripped
                  ? t("safety.summary.tripped")
                  : stops.positions === 0
                    ? t("safety.summary.ofLimit", { limit: usdt(room.limit) })
                    : wouldTrip
                      ? t("safety.summary.wouldTrip")
                      : t("safety.summary.roomForStops")}
              </span>
            </>
          )
        }
      />
      <KpiTile
        label={t("dashboard.kpi.capitalInUse")}
        value={desk.loaded ? usdt(stops.capital) : null}
        note={r && r.balance > 0 ? t("dashboard.ofBalance", { pct: pct(stops.capital, r.balance) }) : undefined}
      />
      <KpiTile
        label={t("safety.summary.strategyBudget")}
        value={sr ? `${usdt(sr.reserved)}` : null}
        note={
          sr ? (
            <>
              <Meter fraction={sr.cap > 0 ? sr.reserved / sr.cap : 0} label={t("safety.summary.strategyBudget")} />
              <span>{t("safety.summary.budgetNote", { cap: usdt(sr.cap), free: usdt(sr.free) })}</span>
            </>
          ) : undefined
        }
      />
      <KpiTile
        label={t("safety.summary.breakerLeft")}
        value={sr ? (strategy.risk?.tripped ? NO_VALUE : usdt(sr.ddLeft)) : null}
        tone={strategy.risk?.tripped ? "down" : undefined}
        note={
          sr ? (
            <>
              <Meter fraction={sr.ddLimit > 0 ? sr.ddUsed / sr.ddLimit : 0} label={t("safety.summary.breakerLeft")} />
              <span>
                {strategy.risk?.tripped
                  ? t("strategy.notes.portfolioDdTripped")
                  : t("safety.summary.breakerNote", { limit: usdt(sr.ddLimit), bots: strategy.risk?.breakerBots ?? 0 })}
              </span>
            </>
          ) : undefined
        }
      />
    </KpiGrid>
  );
}
