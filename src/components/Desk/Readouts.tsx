import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { Chip, type ChipTone } from "@/components/ui/Chip/Chip";
import { Meter } from "@/components/ui/KpiTile/KpiTile";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatAge, formatLatency, formatPercent, formatPnl, formatSignedUsdt, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { streamErrorText, streamLabelKey, streamTone } from "@/lib/ipc/signal/streamStatus";

/** BTC regime as the engine last read it (bot_status.btcRegime). */
export function RegimeBadge() {
  const { t } = useTranslation();
  const { desk } = useDeskContext();
  const regime = desk.loaded ? desk.status.btcRegime : "unknown";
  return (
    <Chip tone={regime === "normal" ? "success" : regime === "break" ? "danger" : "warn"}>
      {t(`statusbar.regime.${regime}`)}
    </Chip>
  );
}

/** Regime card: state and what it blocks. */
export function RegimeCard() {
  const { t } = useTranslation();
  const { desk } = useDeskContext();
  const regime = desk.loaded ? desk.status.btcRegime : "unknown";
  return (
    <Panel title={t("safety.regime.title")} aside={<RegimeBadge />}>
      <p className="ae-muted">{t(`safety.regime.effect.${regime}`)}</p>
    </Panel>
  );
}

/** Stream state -> chip tone. */
const CHIP_TONE: Record<ReturnType<typeof streamTone>, ChipTone> = {
  success: "success",
  warn: "warn",
  danger: "danger",
  muted: "neutral",
};

/** Sentinel stream status with reconnect. */
export function StreamCard() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { feed } = useDeskContext();
  const connected = feed.health.connected;
  const streamError = streamErrorText(feed.health);
  return (
    <Panel
      title={t("statusbar.stream")}
      aside={
        <Chip tone={!feed.loaded ? "neutral" : CHIP_TONE[streamTone(feed.health)]}>
          {!feed.loaded ? t("statusbar.connecting") : t(streamLabelKey(feed.health))}
        </Chip>
      }
    >
      <FactList
        rows={[
          {
            label: t("statusbar.lastEventLabel"),
            value:
              feed.health.lastSignalSecs != null
                ? formatAge(feed.health.lastSignalSecs, locale)
                : t("statusbar.waitingFirstEvent"),
          },
          ...(feed.health.latencyMs != null ? [{ label: t("statusbar.latency"), value: formatLatency(feed.health.latencyMs, locale) }] : []),
        ]}
      />
      {streamError ? <p className="ae-error">{streamError}</p> : null}
      {feed.loaded && !connected ? (
        <button type="button" className="ae-link" onClick={() => void feed.reconnect()}>
          {t("statusbar.reconnect")}
        </button>
      ) : null}
    </Panel>
  );
}

/**
 * Compact risk level readout, linking to #/risk. Each row names the bots it
 * covers: the position cap and the daily stop hold signal bots only; DCA and
 * Grid bots sit under their own budget cap and portfolio breaker.
 */
export function RiskReadout() {
  const { t } = useTranslation();
  const { risk, strategy } = useDeskContext();
  const s = risk.state;
  const sr = strategy.risk;
  return (
    <Panel
      title={t("safety.level.title")}
      aside={
        <Link to="/risk" className="ae-link ae-muted">
          {t("nav.risk")}
        </Link>
      }
    >
      {s ? (
        <>
          <span className="ae-risklevel" data-level={s.limits.level}>
            {t(`risk.levels.${s.limits.level}`)}
          </span>
          <FactList
            rows={[
              { label: t("safety.scope.leverageAll"), value: `${s.limits.maxLeverage}x` },
              { label: t("safety.scope.positionsSignal"), value: s.limits.maxConcurrentPositions },
              { label: t("safety.scope.dailyStopSignal"), value: `${s.effectiveDailyLossPct}%` },
              ...(sr
                ? [
                    { label: t("safety.scope.strategyBudget"), value: `${sr.budgetCapPct}%` },
                    { label: t("safety.scope.strategyBreaker"), value: `${sr.portfolioDdPct}%` },
                  ]
                : []),
            ]}
          />
        </>
      ) : risk.error ? (
        <p className="ae-error">{risk.error}</p>
      ) : (
        <p className="ae-subtle">{t("workspace.loading")}</p>
      )}
    </Panel>
  );
}

/**
 * The daily stop: tripped or not, and today's realized result against the
 * limit. Realized only (trades_stats.todayPnlQuote); floating loss is not
 * part of this stop today.
 */
export function KillSwitchCard() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk, pnl } = useDeskContext();
  const tripped = desk.loaded && desk.status.killSwitchTripped;
  const s = risk.state;
  const limit = s ? (s.balance * s.effectiveDailyLossPct) / 100 : null;
  const today = pnl.stats?.todayPnlQuote ?? null;
  const used = limit && today !== null && today < 0 ? -today / limit : 0;

  return (
    <Panel
      title={t("safety.killSwitch.title")}
      tone={tripped ? "danger" : "default"}
      aside={<Chip tone={tripped ? "danger" : "success"}>{t(tripped ? "safety.killSwitch.tripped" : "safety.killSwitch.ok")}</Chip>}
    >
      <FactList
        rows={[
          {
            label: pnl.split ? t("safety.killSwitch.realizedTodayScoped", { scope: t(`pnl.scope.${pnl.scope}`) }) : t("safety.killSwitch.realizedToday"),
            value: today === null ? NO_VALUE : formatPnl(today, locale).text,
            tone: pnlToneAttr(today),
          },
          {
            label: t("safety.killSwitch.limit"),
            value: limit === null ? NO_VALUE : `${formatSignedUsdt(-limit, locale)} (${formatPercent(s?.effectiveDailyLossPct ?? 0, locale, 1)})`,
          },
        ]}
      />
      {limit ? <Meter fraction={used} label={t("safety.killSwitch.meter")} /> : null}
      <p className="ae-subtle">{t(tripped ? "safety.killSwitch.resets" : "safety.killSwitch.scope")}</p>
    </Panel>
  );
}
