import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Dot, type DotTone } from "@/components/ui/Chip/Chip";
import { FactList } from "@/components/ui/Panel/Panel";
import { Tooltip } from "@/components/ui/Tooltip/Tooltip";
import { localeForLanguage } from "@/i18n";
import { formatAge, formatLatency, formatPrice } from "@/lib/format";
import type { ExchangeHealth, HealthLevel, HealthSnapshot } from "@/lib/ipc/health/health";
import { streamErrorText, streamLabelKey, streamTone } from "@/lib/ipc/signal/streamStatus";

/**
 * The BTC price Rust reported, or null when the macro feed did not answer.
 * get_health_snapshot starts from an empty pulse (price 0, no sparkline) and
 * only overwrites it with Sentinel's real reading, so "no price" is simply a
 * non-positive value: nothing synthesized ever reaches the snapshot.
 */
export function realBtcPrice(h: HealthSnapshot | null): number | null {
  if (!h) return null;
  return Number.isFinite(h.btc.price) && h.btc.price > 0 ? h.btc.price : null;
}

const levelTone = (l: HealthLevel): DotTone =>
  l === "ok" ? "success" : l === "warn" ? "warn" : l === "down" ? "danger" : "muted";

const LEVEL_RANK: Record<HealthLevel, number> = { down: 3, warn: 2, unknown: 1, ok: 0 };

/**
 * The exchange list folded into one reading: the worst level (one dot), how
 * many answered ok, and the slowest latency among them.
 */
export function summarizeExchanges(list: ExchangeHealth[]): {
  total: number;
  ok: number;
  worst: HealthLevel;
  maxLatencyMs: number | null;
} {
  let worst: HealthLevel = "ok";
  let ok = 0;
  let maxLatencyMs: number | null = null;
  for (const e of list) {
    if (LEVEL_RANK[e.level] > LEVEL_RANK[worst]) worst = e.level;
    if (e.level === "ok") ok += 1;
    if (e.latencyMs != null && (maxLatencyMs === null || e.latencyMs > maxLatencyMs)) maxLatencyMs = e.latencyMs;
  }
  return { total: list.length, ok, worst: list.length === 0 ? "unknown" : worst, maxLatencyMs };
}

/**
 * Bottom status bar: one line that fits the 1180 px default window.
 * [exchanges · max latency] | [Sentinel · last signal] | [BTC price regime] … [vault] | [membership] | [version]
 * Every item keeps its detail in a tooltip.
 */
export function StatusBar() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { health, feed, desk, vault, membership, version } = useDeskContext();
  const price = realBtcPrice(health);
  const regime = desk.loaded ? desk.status.btcRegime : "unknown";
  const member = membership.view.state;
  const ex = health ? summarizeExchanges(health.exchanges) : null;
  const levelText = (l: HealthLevel) =>
    t(`health.status${l === "ok" ? "Ok" : l === "warn" ? "Warn" : l === "down" ? "Down" : "Unknown"}`);

  const streamState = !feed.loaded ? t("statusbar.connecting") : t(streamLabelKey(feed.health));
  const lastSignal =
    feed.health.lastSignalSecs != null ? formatAge(feed.health.lastSignalSecs, locale) : null;
  const streamError = streamErrorText(feed.health);

  return (
    <footer className="ae-statusbar" aria-label={t("statusbar.label")}>
      {ex === null ? (
        <span className="ae-statusbar__item">{t("statusbar.connecting")}</span>
      ) : (
        <Tooltip
          placement="top"
          align="start"
          content={
            <>
              <span className="ae-tip__title">{t("statusbar.exchanges", { count: ex.total })}</span>
              <FactList
                rows={health!.exchanges.map((e) => ({
                  label: e.name,
                  value: e.latencyMs != null ? `${levelText(e.level)} · ${formatLatency(e.latencyMs, locale)}` : levelText(e.level),
                }))}
              />
            </>
          }
        >
          <span className="ae-statusbar__item" tabIndex={0}>
            <Dot tone={levelTone(ex.worst)} />
            {t("statusbar.exchanges", { count: ex.total })}
            {ex.maxLatencyMs != null ? (
              <span className="tabular ae-statusbar__muted ae-statusbar__metric">
                {t("statusbar.maxLatency", { value: formatLatency(ex.maxLatencyMs, locale) })}
              </span>
            ) : null}
          </span>
        </Tooltip>
      )}

      <span className="ae-statusbar__sep" aria-hidden="true" />
      <Tooltip
        placement="top"
        content={
          <>
            <span className="ae-tip__title">Sentinel</span>
            <FactList
              rows={[
                { label: t("cockpit.stream"), value: streamState },
                ...(lastSignal ? [{ label: t("statusbar.lastEventLabel"), value: lastSignal }] : []),
                ...(feed.health.latencyMs != null
                  ? [{ label: t("statusbar.latency"), value: formatLatency(feed.health.latencyMs, locale) }]
                  : []),
              ]}
            />
            {streamError ? <span className="ae-cockpit__hint">{streamError}</span> : null}
          </>
        }
      >
        <span className="ae-statusbar__item" tabIndex={0}>
          <Dot tone={streamTone(feed.health, feed.loaded)} />
          Sentinel
          <span className="ae-statusbar__muted ae-statusbar__metric tabular">
            {feed.loaded && feed.health.connected
              ? lastSignal
                ? t("statusbar.lastSignalShort", { age: lastSignal })
                : t("statusbar.waitingFirstEvent")
              : streamState}
          </span>
        </span>
      </Tooltip>

      <span className="ae-statusbar__sep ae-statusbar__wide" aria-hidden="true" />
      <span className="ae-statusbar__item ae-statusbar__wide">
        BTC
        <span className="tabular">{price !== null ? formatPrice(price, locale) : t("statusbar.priceUnavailable")}</span>
        <span className="ae-statusbar__regime" data-regime={regime}>
          {t(`statusbar.regime.${regime}`)}
        </span>
      </span>

      <span className="ae-statusbar__spacer" />

      <Tooltip
        placement="top"
        align="end"
        content={
          vault.status.idleTimeoutMinutes > 0 ? t("statusbar.autoLock", { minutes: vault.status.idleTimeoutMinutes }) : null
        }
      >
        <span className="ae-statusbar__item ae-statusbar__wide" tabIndex={vault.status.idleTimeoutMinutes > 0 ? 0 : undefined}>
          <Dot tone={vault.status.state === "unlocked" ? "success" : "warn"} />
          {t(vault.status.state === "unlocked" ? "health.vaultUnlocked" : "health.vaultLocked")}
        </span>
      </Tooltip>
      <span className="ae-statusbar__sep ae-statusbar__wide" aria-hidden="true" />
      <span className="ae-statusbar__item">
        <Dot tone={member === "active" ? "success" : member === "expiring" ? "warn" : "danger"} />
        {t(`membership.state.${member}`)}
      </span>
      {version ? (
        <>
          <span className="ae-statusbar__sep ae-statusbar__wide" aria-hidden="true" />
          <span className="ae-statusbar__item ae-statusbar__muted ae-statusbar__wide">v{version}</span>
        </>
      ) : null}
    </footer>
  );
}
