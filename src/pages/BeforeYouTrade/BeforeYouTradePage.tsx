import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { InfoTip } from "@/components/ui/InfoTip/InfoTip";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { errorMessage } from "@/lib/ipc/bridge";
import { formatAge, formatNumber, formatPercent, formatPrice, formatSignedPercent, formatTableTime, NO_VALUE } from "@/lib/format";
import {
  beforeYouTrade,
  openSourceLink,
  type BeforeYouTradeView,
  type ChartRead,
  type FarmTemplate,
  type GhostTemplate,
  type ImpactType,
  type LabTemplate,
  type Leaf,
  type ListingRow,
  type NewsLine,
} from "@/lib/ipc/beforeYouTrade/beforeYouTrade";
import {
  ageSecs,
  chartReadHiddenReason,
  dominanceLabelKey,
  farmRankingShown,
  impactLine,
  isProven,
  reactionCell,
  statusTone,
  templateFormPath,
  templatesInOrder,
  testSplit,
} from "./bytModel";
import "./BeforeYouTrade.css";

type Locale = string;

function useNowTick(ms = 30_000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), ms);
    return () => window.clearInterval(id);
  }, [ms]);
  return now;
}

/** A leaf's status chip with its as-of age. */
function LeafHead({ leaf, now }: { leaf: Pick<Leaf, "status" | "as_of">; now: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const age = ageSecs(leaf.as_of, now);
  return (
    <span className="ae-byt__head">
      {leaf.as_of ? (
        <span className="ae-subtle tabular" title={leaf.as_of}>
          {t("byt.asOf", { time: formatTableTime(leaf.as_of, locale, now), age: age === null ? NO_VALUE : formatAge(age, locale) })}
        </span>
      ) : null}
      <Chip tone={statusTone(leaf.status)}>{t(`byt.status.${leaf.status}`, { defaultValue: leaf.status })}</Chip>
    </span>
  );
}

/** The leaf's reason when its figures are not there. */
function Reason({ leaf }: { leaf: Pick<Leaf, "status" | "reason"> }) {
  const { t } = useTranslation();
  if (leaf.status === "ok" && !leaf.reason) return null;
  return <p className="ae-byt__reason">{leaf.reason || t("byt.noData")}</p>;
}

function SourceLink({ url, children }: { url: string; children: ReactNode }) {
  const { t } = useTranslation();
  const [failed, setFailed] = useState(false);
  if (!url) return <>{children}</>;
  return (
    <button
      type="button"
      className="ae-link ae-byt__linkbtn"
      title={failed ? t("byt.linkFailed") : url}
      onClick={() => {
        openSourceLink(url).catch(() => setFailed(true));
      }}
    >
      {children}
    </button>
  );
}

const pct = (v: number | null | undefined, locale: Locale, digits = 2) => (v === null || v === undefined ? NO_VALUE : formatPercent(v, locale, digits));
const spct = (v: number | null | undefined, locale: Locale, digits = 2) =>
  v === null || v === undefined ? NO_VALUE : formatSignedPercent(v, locale, digits);
const num = (v: number | null | undefined, locale: Locale, digits = 2) =>
  v === null || v === undefined ? NO_VALUE : formatNumber(v, locale, { maximumFractionDigits: digits });

// ---------------------------------------------------------------- Market today

function ChartReadBlock({ r, now }: { r: ChartRead; now: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const hidden = chartReadHiddenReason(r);
  const hr = r.hit_record_24h;
  return (
    <div className="ae-byt__sub" data-testid="byt-chart-read">
      <div className="ae-byt__subhead">
        <h3>{t("byt.chart.title")}</h3>
        <LeafHead leaf={r} now={now} />
      </div>
      {hidden ? (
        <p className="ae-byt__reason">{r.reason || t(hidden)}</p>
      ) : (
        <>
          <FactList
            rows={[
              { label: t("byt.chart.call"), value: t(`byt.chart.calls.${r.call}`), info: r.use },
              { label: t("byt.chart.confidence"), value: r.confidence || NO_VALUE },
              {
                label: t("byt.chart.horizons"),
                value: r.horizon_view
                  ? t("byt.chart.horizonLine", {
                      h4: t(`byt.chart.calls.${r.horizon_view.h4 || "none"}`),
                      h12: t(`byt.chart.calls.${r.horizon_view.h12 || "none"}`),
                      h24: t(`byt.chart.calls.${r.horizon_view.h24 || "none"}`),
                    })
                  : NO_VALUE,
              },
              { label: t("byt.chart.priceAtRead"), value: r.price_at_read === null ? NO_VALUE : formatPrice(r.price_at_read, locale) },
              { label: t("byt.chart.support"), value: r.support.length ? r.support.map((v) => formatPrice(v, locale)).join(", ") : NO_VALUE },
              { label: t("byt.chart.resistance"), value: r.resistance.length ? r.resistance.map((v) => formatPrice(v, locale)).join(", ") : NO_VALUE },
              { label: t("byt.chart.invalidation"), value: r.invalidation_level === null ? NO_VALUE : formatPrice(r.invalidation_level, locale) },
              { label: t("byt.chart.model"), value: `${r.model} · ${r.prompt_version}`, tone: "muted" },
            ]}
          />
          {hr ? (
            <p className="ae-byt__record" data-testid="byt-hit-record">
              {t("byt.chart.record", {
                hit: pct(hr.hit_pct, locale, 1),
                hits: hr.hits,
                n: hr.n,
                days: hr.days,
                range: pct(hr.always_range_pct, locale, 1),
                persistence: pct(hr.persistence_pct, locale, 1),
              })}{" "}
              {t("byt.chart.directional", { pct: pct(hr.directional_pct, locale, 1), n: hr.directional_n })}
            </p>
          ) : null}
          {r.bar ? (
            <p className="ae-byt__reason" data-tone={r.bar.status === "met" ? undefined : "warn"}>
              {r.bar.verdict} {r.use}
            </p>
          ) : null}
        </>
      )}
      {r.latest_attempt_status && r.latest_attempt_status !== "ok" && r.latest_attempt_at ? (
        <p className="ae-subtle">
          {t("byt.chart.latestAttempt", { time: formatTableTime(r.latest_attempt_at, locale, now), status: r.latest_attempt_status })}
        </p>
      ) : null}
    </div>
  );
}

function MarketBlock({ v, now }: { v: BeforeYouTradeView; now: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const m = v.payload.market;
  const b = m.btc_regime;
  const g = b.scanner_guard;
  const regimeRows: Fact[] =
    b.status === "ok"
      ? [
          { label: t("byt.market.price"), value: b.price === null ? NO_VALUE : formatPrice(b.price, locale) },
          { label: t("byt.market.change24h"), value: spct(b.change_24h_pct, locale) },
          { label: t("byt.market.change1h"), value: spct(b.change_1h_pct, locale) },
          {
            label: t("byt.market.trend"),
            value: b.trend_state
              ? t("byt.market.trendValue", { state: t(`byt.market.trendStates.${b.trend_state}`), strength: num(b.trend_strength, locale) })
              : NO_VALUE,
            info: b.trend_rule,
          },
          { label: t("byt.market.volatility"), value: b.volatility_regime ? t(`byt.market.volStates.${b.volatility_regime}`) : NO_VALUE, info: b.volatility_rule },
          { label: t(dominanceLabelKey(b.dominance_kind)), value: pct(b.btc_dominance_pct, locale), info: t("byt.market.dominanceSource", { source: b.dominance_source }) },
          { label: t("byt.market.altLongRisk"), value: num(b.alt_long_risk, locale) },
          { label: t("byt.market.downDays"), value: b.down_day_streak ?? NO_VALUE },
          {
            label: t("byt.market.downDayHalt"),
            value: b.down_day_halt_active === null ? NO_VALUE : t(b.down_day_halt_active ? "byt.market.active" : "byt.market.inactive"),
          },
          {
            label: t("byt.market.newAltLongs"),
            value: g
              ? g.new_alt_longs_blocked
                ? g.sticky_until
                  ? t("byt.market.blockedUntil", { time: formatTableTime(g.sticky_until, locale, now) })
                  : t("byt.market.blocked")
                : t("byt.market.published")
              : NO_VALUE,
            info: g?.rule,
            tone: g?.new_alt_longs_blocked ? "warn" : undefined,
          },
        ]
      : [];
  const range = m.volatility.range_24h;
  const atr = m.volatility.atr14_1d;
  const f = m.funding;
  const fundingCols: DataColumn<(typeof f.rows)[number]>[] = [
    { id: "symbol", header: t("table.symbol"), cell: (r) => r.symbol },
    { id: "rate", header: t("byt.funding.rate"), numeric: true, cell: (r) => t("byt.funding.perInterval", { rate: formatSignedPercent(r.rate_pct, locale, 4), hours: r.interval_hours }) },
    { id: "annual", header: t("byt.funding.annualized"), numeric: true, priority: 2, cell: (r) => formatSignedPercent(r.annualized_pct, locale, 2) },
    { id: "next", header: t("byt.funding.next"), priority: 2, cell: (r) => formatTableTime(r.next_funding, locale, now) },
  ];
  return (
    <Panel title={t("byt.market.title")} aside={<LeafHead leaf={b} now={now} />}>
      <div className="ae-byt__grid">
        <div className="ae-byt__sub">
          <div className="ae-byt__subhead">
            <h3>{t("byt.market.regime")}</h3>
          </div>
          {b.status === "ok" ? <FactList rows={regimeRows} /> : <Reason leaf={b} />}
        </div>
        <ChartReadBlock r={m.chart_read} now={now} />
      </div>
      <div className="ae-byt__grid">
        <div className="ae-byt__sub">
          <div className="ae-byt__subhead">
            <h3>{t("byt.vol.title")}</h3>
            <LeafHead leaf={atr} now={now} />
          </div>
          <FactList
            rows={[
              range.status === "ok"
                ? { label: t("byt.vol.range24h"), value: t("byt.vol.rangeValue", { low: range.low === null ? NO_VALUE : formatPrice(range.low, locale), high: range.high === null ? NO_VALUE : formatPrice(range.high, locale), pct: pct(range.range_pct, locale) }) }
                : { label: t("byt.vol.range24h"), value: range.reason || t("byt.noData"), tone: "muted" },
              atr.status === "ok"
                ? {
                    label: t("byt.vol.atr"),
                    value: t("byt.vol.atrValue", { usd: num(atr.atr_usd, locale, 0), pct: pct(atr.atr_pct, locale), n: atr.bars_used }),
                    info: atr.method,
                  }
                : { label: t("byt.vol.atr"), value: atr.reason || t("byt.noData"), tone: "muted" },
            ]}
          />
        </div>
        <div className="ae-byt__sub">
          <div className="ae-byt__subhead">
            <h3>{t("byt.funding.title")}</h3>
            <LeafHead leaf={f} now={now} />
          </div>
          {f.status === "ok" ? (
            <>
              <DataTable label={t("byt.funding.title")} columns={fundingCols} rows={f.rows} rowKey={(r) => r.symbol} compact bare empty={<p className="ae-subtle">{t("byt.noData")}</p>} />
              {f.datahub_stale ? <p className="ae-byt__reason" data-tone="warn">{t("byt.funding.stale")}</p> : null}
            </>
          ) : (
            <Reason leaf={f} />
          )}
        </div>
      </div>
    </Panel>
  );
}

// ---------------------------------------------------------------- Bot models

function templateName(t: TFunction, presetId: string, fallback?: string): string {
  return t(`strategy.preset.names.${presetId}`, { defaultValue: fallback || presetId });
}

function BotModelsBlock({ v, now }: { v: BeforeYouTradeView; now: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const p = v.payload.presets;
  const ghost = p.ghost;
  const farm = p.farm;
  const ghostById = new Map<string, GhostTemplate>((ghost.templates ?? []).map((g) => [g.template_id, g]));
  const farmById = new Map<string, FarmTemplate>((farm.templates ?? []).map((g) => [g.template_id, g]));
  const ranking = farm.labels?.ranking;
  const rows = templatesInOrder(p);

  const cols: DataColumn<LabTemplate>[] = [
    {
      id: "name",
      header: t("byt.models.template"),
      keep: true,
      wrap: true,
      cell: (r) => (
        <span className="ae-byt__tcell">
          <span className="ae-byt__tname">
            <span className="ae-subtle tabular ae-byt__tid">{r.template_id}</span>
            <button
              type="button"
              className="ae-link ae-byt__linkbtn"
              title={t("byt.models.open")}
              onClick={() => navigate(templateFormPath(r))}
            >
              {templateName(t, r.preset_id, farmById.get(r.template_id)?.name)}
            </button>
          </span>
          <Chip tone={isProven(r) ? "success" : "warn"}>{t(isProven(r) ? "byt.models.proven" : "byt.models.experimental")}</Chip>
        </span>
      ),
    },
    {
      id: "universe",
      header: t("byt.models.universe"),
      priority: 3,
      cell: (r) => (r.universe.rule === "btcOnly" ? t("byt.models.btcOnly") : t("byt.models.topN", { from: r.universe.rank_from, to: r.universe.rank_from + r.universe.top_n - 1 })),
    },
    {
      id: "test",
      header: t("byt.models.test"),
      headerTip: t("byt.models.testTip"),
      numeric: true,
      cell: (r) => {
        const s = testSplit(r);
        return s ? t("byt.models.testValue", { mean: spct(s.mean_per_bot_month_pct, locale), lo: spct(s.ci95_pct[0], locale), hi: spct(s.ci95_pct[1], locale) }) : NO_VALUE;
      },
    },
    {
      id: "months",
      header: t("byt.models.months"),
      numeric: true,
      priority: 2,
      cell: (r) => {
        const s = testSplit(r);
        return s ? t("byt.models.monthsValue", { pos: s.months_positive, n: s.months }) : NO_VALUE;
      },
    },
    { id: "bots", header: t("byt.models.testBots"), numeric: true, priority: 2, cell: (r) => r.test_bots },
    { id: "dd", header: t("byt.models.worstDd"), numeric: true, priority: 3, cell: (r) => spct(r.test_worst_bot_drawdown_pct, locale, 1) },
    {
      id: "ghost",
      header: t("byt.models.ghost"),
      headerTip: t("byt.models.ghostTip"),
      numeric: true,
      priority: 2,
      cell: (r) => {
        const g = ghostById.get(r.template_id);
        if (ghost.status !== "ok" && ghost.status !== "measuring") return NO_VALUE;
        return g ? t("byt.models.ghostValue", { ret: spct(g.return_pct, locale), bots: g.bots, cycles: g.cycles }) : NO_VALUE;
      },
    },
    {
      id: "farm",
      header: t("byt.models.farm"),
      headerTip: t("byt.models.farmTip"),
      numeric: true,
      priority: 3,
      cell: (r) => {
        const g = farmById.get(r.template_id);
        if (!g || (farm.status !== "ok" && farm.status !== "measuring")) return NO_VALUE;
        return t("byt.models.farmValue", { ret: spct(g.return_pct, locale), closed: g.cycles_closed, open: g.cycles_open });
      },
    },
  ];

  return (
    <Panel title={t("byt.models.title")} aside={<LeafHead leaf={p.lab} now={now} />}>
      <p className="ae-byt__lead">{p.claim_rule}</p>
      <p className="ae-subtle">{t("byt.models.openHint")}</p>
      {p.lab.status === "ok" ? (
        <DataTable
          label={t("byt.models.title")}
          columns={cols}
          rows={rows}
          rowKey={(r) => r.template_id}
          compact
          rowNote={(r) => (r.template_id === p.lab.majors_caveat.template_id ? p.lab.majors_caveat.text : null)}
        />
      ) : (
        <Reason leaf={p.lab} />
      )}
      <p className="ae-subtle">{p.order} {p.lab.recheck.text}</p>

      <div className="ae-byt__grid">
        <div className="ae-byt__sub" data-testid="byt-ghost">
          <div className="ae-byt__subhead">
            <h3>{t("byt.ghost.title")}</h3>
            <LeafHead leaf={ghost} now={now} />
          </div>
          <p>
            {ghost.days_running === null
              ? t("byt.noData")
              : t("byt.ghost.measuring", { days: num(ghost.days_running, locale, 1), min: ghost.min_days_for_ranking })}
          </p>
          <Reason leaf={ghost} />
          <p className="ae-subtle">{t("byt.ghost.notRanked")} {ghost.method}</p>
        </div>
        <div className="ae-byt__sub" data-testid="byt-farm">
          <div className="ae-byt__subhead">
            <h3>{t("byt.farm.title")}</h3>
            <LeafHead leaf={farm} now={now} />
          </div>
          {farm.status === "not_started" || farm.status === "unavailable" ? (
            <Reason leaf={farm} />
          ) : (
            <>
              <FactList
                rows={[
                  { label: t("byt.farm.days"), value: farm.days_running === undefined ? NO_VALUE : t("byt.farm.daysValue", { days: num(farm.days_running, locale, 2), min: ranking?.min_days ?? 30 }) },
                  { label: t("byt.farm.mode"), value: t("byt.farm.paperSimulated") },
                  { label: t("byt.farm.bots"), value: farm.bot_count ?? NO_VALUE },
                  { label: t("byt.farm.refused"), value: farm.refused_count ?? NO_VALUE },
                ]}
              />
              {farm.labels ? <p className="ae-subtle">{farm.labels.text}</p> : null}
              {farmRankingShown(ranking) ? (
                <ol className="ae-byt__ranked" data-testid="byt-farm-ranking">
                  {ranking!.ranked.map((x) => (
                    <li key={x.unit}>
                      <span>{x.unit}</span> <span className="tabular">{spct(x.return_pct, locale)}</span>
                    </li>
                  ))}
                </ol>
              ) : (
                <div className="ae-byt__reason" data-testid="byt-farm-noranking">
                  <p>{t("byt.farm.noRanking")}</p>
                  {ranking?.reasons.length ? (
                    <ul>
                      {ranking.reasons.map((r) => (
                        <li key={r}>{r}</li>
                      ))}
                    </ul>
                  ) : null}
                </div>
              )}
            </>
          )}
        </div>
      </div>
      <div className="ae-byt__sub">
        <div className="ae-byt__subhead">
          <h3>{t("byt.similar.title")}</h3>
          <LeafHead leaf={p.similar_days} now={now} />
        </div>
        <Reason leaf={p.similar_days} />
      </div>
    </Panel>
  );
}

// ---------------------------------------------------------------- News

function ImpactRow({ type }: { type: ImpactType }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  if (type.status === "unavailable") return <li>{t("byt.news.impactUnavailable", { label: type.label, reason: type.reason || t("byt.noData") })}</li>;
  const l = impactLine(type.horizons["24h"]);
  if (l.kind === "measuring") return <li>{t("byt.news.impactMeasuring", { label: type.label, n: l.n, needed: l.needed ?? 30 })}</li>;
  if (l.kind === "none") return <li>{t("byt.news.impactNone", { label: type.label })}</li>;
  return (
    <li>
      {t("byt.news.impactMeasured", { label: type.label, median: spct(l.median, locale), up: pct(l.shareUp, locale, 1), n: l.n, base: pct(l.baseShareUp, locale, 1) })}{" "}
      {l.differs && l.ci
        ? t("byt.news.differs", { lo: spct(l.ci[0], locale), hi: spct(l.ci[1], locale) })
        : t("byt.news.noDifference")}
    </li>
  );
}

function NewsBlock({ v, now }: { v: BeforeYouTradeView; now: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const n = v.payload.news;
  const types = n.impact.types ?? [];
  const lineCols: DataColumn<NewsLine>[] = [
    { id: "text", header: t("byt.news.line"), wrap: true, keep: true, cell: (l) => l.text || t("byt.news.withheldText") },
    {
      id: "source",
      header: t("byt.news.source"),
      priority: 2,
      cell: (l) => <SourceLink url={l.url}>{l.source_name || t("byt.news.sourceLink")}</SourceLink>,
    },
    { id: "time", header: t("table.time"), priority: 3, width: "1%", cell: (l) => (l.published_at ? formatTableTime(l.published_at, locale, now) : NO_VALUE) },
  ];
  return (
    <Panel title={t("byt.news.title")} aside={<LeafHead leaf={n} now={now} />}>
      {n.status === "ok" || n.lines.length ? (
        <DataTable label={t("byt.news.title")} columns={lineCols} rows={n.lines} rowKey={(l) => l.url || l.text} compact empty={<p className="ae-subtle">{t("byt.noData")}</p>} />
      ) : (
        <Reason leaf={n} />
      )}
      <p className="ae-subtle">
        {t("byt.news.count", { shown: n.lines.length, total: n.lines_total })}
        {n.withheld_lines > 0 ? ` ${t("byt.news.withheld", { count: n.withheld_lines })}` : ""} {n.disclosure}
      </p>
      <div className="ae-byt__sub" data-testid="byt-impact">
        <div className="ae-byt__subhead">
          <h3>{t("byt.news.impactTitle", { days: n.impact.window_days })}</h3>
          <LeafHead leaf={n.impact} now={now} />
        </div>
        {n.impact.status === "ok" || types.length ? (
          <ul className="ae-byt__impact">
            {types.map((ty) => (
              <ImpactRow key={ty.type} type={ty} />
            ))}
          </ul>
        ) : (
          <Reason leaf={n.impact} />
        )}
        <p className="ae-subtle">{n.impact.disclosure}</p>
      </div>
    </Panel>
  );
}

// ---------------------------------------------------------------- Listings

function ReactionText({ row }: { row: ListingRow }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const c = reactionCell(row);
  if (c.kind === "noData") return <span className="ae-subtle" title={c.reason ?? undefined}>{t("byt.noData")}{c.reason ? ` · ${c.reason}` : ""}</span>;
  if (c.kind === "pending") return <span className="ae-subtle">{t("byt.listings.pending")}</span>;
  return <span className="tabular">{c.values.map((x) => `${x.horizon} ${spct(x.pct, locale)}`).join(" · ")}</span>;
}

function ListingsBlock({ v, now }: { v: BeforeYouTradeView; now: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const l = v.payload.listings;
  const cols: DataColumn<ListingRow>[] = [
    { id: "symbol", header: t("table.symbol"), keep: true, cell: (r) => (r.pairs.length ? r.pairs.join(", ") : r.symbol) },
    { id: "exchange", header: t("table.exchange"), cell: (r) => r.exchange },
    { id: "market", header: t("table.market"), priority: 2, cell: (r) => t(`byt.listings.markets.${r.market}`, { defaultValue: r.market }) },
    {
      id: "time",
      header: t("table.time"),
      priority: 2,
      cell: (r) =>
        r.cancelled_at
          ? t("byt.listings.cancelled")
          : r.listing_time
            ? `${formatTableTime(r.listing_time, locale, now)}${r.postponed ? ` · ${t("byt.listings.postponed")}` : ""}`
            : NO_VALUE,
    },
    {
      id: "source",
      header: t("byt.news.source"),
      priority: 3,
      cell: (r) => <SourceLink url={r.announcement.url}>{r.announcement.title || r.announcement.source}</SourceLink>,
      wrap: true,
    },
    { id: "reaction", header: t("byt.listings.reaction"), headerTip: l.reaction_method, cell: (r) => <ReactionText row={r} />, wrap: true },
  ];
  const summary = l.reaction_summary ?? [];
  return (
    <Panel title={t("byt.listings.title")} aside={<LeafHead leaf={l} now={now} />}>
      {l.reason ? <p className="ae-byt__reason">{l.reason}</p> : null}
      {l.status === "unavailable" && !l.upcoming.length && !l.recent.length ? null : (
        <>
          <h3 className="ae-byt__h3">{t("byt.listings.upcoming", { shown: l.upcoming.length, total: l.upcoming_total })}</h3>
          <DataTable label={t("byt.listings.upcomingLabel")} columns={cols} rows={l.upcoming} rowKey={(r) => `${r.exchange}:${r.market}:${r.symbol}:${r.listing_time}`} compact empty={<p className="ae-subtle">{t("byt.listings.none")}</p>} />
          <h3 className="ae-byt__h3">{t("byt.listings.recent", { shown: l.recent.length, total: l.recent_total })}</h3>
          <DataTable label={t("byt.listings.recentLabel")} columns={cols} rows={l.recent} rowKey={(r) => `${r.exchange}:${r.market}:${r.symbol}:${r.listing_time}`} compact empty={<p className="ae-subtle">{t("byt.listings.none")}</p>} />
        </>
      )}
      {summary.length ? (
        <div className="ae-byt__sub">
          <div className="ae-byt__subhead">
            <h3>{t("byt.listings.summary")}</h3>
          </div>
          <ul className="ae-byt__impact">
            {summary.map((s) => (
              <li key={`${s.exchange}:${s.market}`}>
                {t("byt.listings.summaryLine", { exchange: s.exchange, market: t(`byt.listings.markets.${s.market}`, { defaultValue: s.market }), listings: s.listings, noData: s.no_data, pending: s.pending })}{" "}
                {s.horizons.some((h) => h.n > 0)
                  ? s.horizons
                      .map((h) => t("byt.listings.horizon", { h: h.horizon, median: spct(h.median_pct, locale), up: pct(h.positive_rate_pct, locale, 0), n: h.n }))
                      .join(" · ")
                  : t("byt.noData")}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      <p className="ae-subtle">
        {t("byt.listings.sources", {
          list: l.sources.map((s) => (s.status === "ok" ? s.id : `${s.id} (${s.status})`)).join(", "),
        })}{" "}
        {l.disclosure}
      </p>
    </Panel>
  );
}

// ---------------------------------------------------------------- Page

export function BeforeYouTradePage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const now = useNowTick();
  const [view, setView] = useState<BeforeYouTradeView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(
    (refresh: boolean) => {
      setLoading(true);
      beforeYouTrade(refresh)
        .then((v) => {
          setView(v);
          setError(null);
        })
        .catch((e: unknown) => setError(errorMessage(e, "bytUnavailable")))
        .finally(() => setLoading(false));
    },
    [t],
  );
  useEffect(() => load(false), [load]);

  const errorText = (code: string) => {
    const [head, detail] = code.split("|");
    return t(`byt.errors.${head}`, { defaultValue: code, detail });
  };

  const refreshBtn = (
    <Button variant="secondary" size="sm" onClick={() => load(true)} disabled={loading}>
      {loading ? t("byt.refreshing") : t("byt.refresh")}
    </Button>
  );

  if (!view) {
    return (
      <PageShell title={t("nav.beforeYouTrade")} single secondary={refreshBtn}>
        {error ? (
          <EmptyState tone="error" title={t("byt.unavailableTitle")} detail={errorText(error)} actions={refreshBtn} />
        ) : (
          <EmptyState tone="pending" title={t("byt.loading")} />
        )}
      </PageShell>
    );
  }

  const p = view.payload;
  // The tick re-renders; the age itself is read now (the tick may predate the copy).
  const age = Math.max(0, (Date.now() - view.fetchedAtMs) / 1000);
  const blocks: { key: string; leaf: Pick<Leaf, "status" | "as_of"> }[] = [
    { key: "byt.market.title", leaf: p.market.btc_regime },
    { key: "byt.chart.title", leaf: p.market.chart_read },
    { key: "byt.models.title", leaf: p.presets.lab },
    { key: "byt.ghost.title", leaf: p.presets.ghost },
    { key: "byt.farm.title", leaf: p.presets.farm },
    { key: "byt.news.title", leaf: p.news },
    { key: "byt.listings.title", leaf: p.listings },
  ];

  const left = (
    <Panel title={t("byt.sources")}>
      <ul className="ae-byt__blocks">
        {blocks.map((b) => (
          <li key={b.key}>
            <span className="ae-byt__blockname">{t(b.key)}</span>
            <LeafHead leaf={b.leaf} now={now} />
          </li>
        ))}
      </ul>
      <p className="ae-subtle">
        {t("byt.received", { age: formatAge(age, locale) })}
        <InfoTip label={t("byt.received", { age: formatAge(age, locale) })} content={`${p.schema} · ${p.generated_at}`} size="sm" />
      </p>
    </Panel>
  );
  const right = (
    <Panel title={t("byt.aboutTitle")}>
      <p className="ae-byt__lead">{p.disclosure}</p>
    </Panel>
  );

  return (
    <PageShell title={t("nav.beforeYouTrade")} secondary={refreshBtn} left={left} right={right}>
      {view.stale ? (
        <div className="ae-banner" data-tone="warn" role="status" data-testid="byt-stale">
          <span>
            {t("byt.staleLine", { age: formatAge(age, locale) })} {view.error ? errorText(view.error) : null}
          </span>
        </div>
      ) : null}
      {error && !view.stale ? (
        <div className="ae-banner" data-tone="warn" role="status">
          <span>{errorText(error)}</span>
        </div>
      ) : null}
      <div className="ae-byt">
        <MarketBlock v={view} now={now} />
        <BotModelsBlock v={view} now={now} />
        <NewsBlock v={view} now={now} />
        <ListingsBlock v={view} now={now} />
      </div>
    </PageShell>
  );
}
