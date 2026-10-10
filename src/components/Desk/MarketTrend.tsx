import { useTranslation } from "react-i18next";
import { Link } from "@/app/router/router";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { Tooltip } from "@/components/ui/Tooltip/Tooltip";
import { localeForLanguage } from "@/i18n";
import { formatPrice, formatSignedPercent } from "@/lib/format";
import { useMarketTrend } from "@/lib/ipc/market/useMarketTrend";
import "./MarketTrend.css";

const FAQ_PATH = "/faq?s=market-trend";

function sinceDate(ms: number, locale: string): string {
  return new Date(ms).toLocaleDateString(locale, { day: "numeric", month: "short", timeZone: "UTC" });
}

/** Header chip: BTC's 50-day trend and its age. Information only. */
export function MarketTrendChip() {
  const { t } = useTranslation();
  const { trend } = useMarketTrend();
  if (!trend) return null;
  return (
    <Tooltip content={t("trend.chipTooltip")} placement="bottom">
      <Link to="/bots" className="ae-trendchip" data-state={trend.state}>
        <span aria-hidden="true">{trend.state === "up" ? "▲" : "▼"}</span>
        {t("trend.chip", { state: t(`trend.state.${trend.state}`), days: trend.days })}
      </Link>
    </Tooltip>
  );
}

/**
 * Side panel: the trend, how it is read, and what the 8 Oct test measured.
 * Nothing here changes a bot.
 */
export function MarketTrendPanel() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { trend, error } = useMarketTrend();
  return (
    <Panel
      title={t("trend.title")}
      aside={trend ? <StatusChip status={trend.state === "up" ? "ok" : "error"} label={t(`trend.state.${trend.state}`)} /> : null}
    >
      {trend ? (
        <>
          <FactList
            rows={[
              { label: t("trend.since"), value: `${sinceDate(trend.sinceMs, locale)} · ${t("trend.days", { count: trend.days })}` },
              { label: t("trend.close"), value: formatPrice(trend.close, locale) },
              { label: t("trend.sma50"), value: formatPrice(trend.sma50, locale) },
              { label: t("trend.gap"), value: formatSignedPercent(trend.gapPct, locale), tone: trend.gapPct >= 0 ? "up" : "down" },
              { label: t("trend.switches"), value: trend.switches365d, info: t("trend.switchesInfo") },
            ]}
          />
          <p className="ae-subtle">{t("trend.rule")}</p>
          <p className="ae-trend__history">{t("trend.history.dca")}</p>
          <p className="ae-trend__history">{t("trend.history.signal")}</p>
          <p className="ae-subtle">
            {t("trend.infoOnly")}{" "}
            <Link to={FAQ_PATH} className="ae-link">
              {t("trend.more")}
            </Link>
          </p>
        </>
      ) : error ? (
        <p className="ae-error">{error}</p>
      ) : (
        <p className="ae-subtle">{t("workspace.loading")}</p>
      )}
    </Panel>
  );
}

/**
 * Form note: shown when the bot's direction runs against the trend. `long`
 * and `short` say which sides the bot can open.
 */
export function TrendMismatchNote({ long, short }: { long: boolean; short: boolean }) {
  const { t } = useTranslation();
  const { trend } = useMarketTrend();
  if (!trend) return null;
  const against = (trend.state === "down" && long && !short) || (trend.state === "up" && short && !long);
  return (
    <p className="ae-banner" data-tone={against ? "warn" : "info"}>
      {against
        ? t(trend.state === "down" ? "trend.warn.longInDown" : "trend.warn.shortInUp", { days: trend.days })
        : t("trend.formNow", { state: t(`trend.state.${trend.state}`), days: trend.days })}{" "}
      <Link to={FAQ_PATH} className="ae-link">
        {t("trend.more")}
      </Link>
    </p>
  );
}
