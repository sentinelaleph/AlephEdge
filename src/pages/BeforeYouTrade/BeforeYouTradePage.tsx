import { Fragment, useCallback, useEffect, useState, type ReactNode } from "react";
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
  type FarmSignalBot,
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
  newsTranslation,
  reactionCell,
  statusTone,
  templateFormPath,
  templatesInOrder,
  testSplit,
} from "./bytModel";
import {
  attemptStatusText,
  barText,
  chartUseText,
  claimRuleText,
  confidenceText,
  disclosureFacts,
  farmBotStateText,
  farmMarketText,
  farmRankReasons,
  feedStatusText,
  impactTypeName,
  majorsCaveatText,
  reasonText,
  recheckText,
  refusalText,
  sayCoded,
  sayText,
  statusText,
  type HasReason,
} from "./bytText";
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
      <Chip tone={statusTone(leaf.status)}>{statusText(t, leaf.status)}</Chip>
    </span>
  );
}

/** The leaf's reason when its figures are not there, in the reader's language. */
function Reason({ leaf }: { leaf: Pick<Leaf, "status"> & HasReason }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  if (leaf.status === "ok" && !leaf.reason && !leaf.reason_code) return null;
  return <p className="ae-byt__reason">{reasonText(t, leaf, locale) ?? t("byt.noData")}</p>;
}

function SourceLink({ url, hint, children }: { url: string; hint?: string; children: ReactNode }) {
  const { t } = useTranslation();
  const [failed, setFailed] = useState(false);
  if (!url) return <>{children}</>;
  return (
    <button
      type="button"
      className="ae-link ae-byt__linkbtn"
      title={failed ? t("byt.linkFailed") : hint || url}
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

/** One full-width row of a block: a heading with its status, then its content. */
function Row({ title, leaf, now, testId, children }: { title: string; leaf?: Pick<Leaf, "status" | "as_of">; now: number; testId?: string; children: ReactNode }) {
  return (
    <section className="ae-byt__row" data-testid={testId}>
      <div className="ae-byt__subhead">
        <h3>{title}</h3>
        {leaf ? <LeafHead leaf={leaf} now={now} /> : null}
      </div>
      {children}
    </section>
  );
}

function ChartReadBlock({ r, now }: { r: ChartRead; now: number }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const hidden = chartReadHiddenReason(r);
  const hr = r.hit_record_24h;
  const use = chartUseText(t, r);
  const bar = r.bar ? barText(t, r.bar, locale) : null;
  return (
    <Row title={t("byt.chart.title")} leaf={r} now={now} testId="byt-chart-read">
      {hidden ? (
        <p className="ae-byt__reason">{reasonText(t, r, locale) ?? t(hidden)}</p>
      ) : (
        <>
          <FactList
            rows={[
              { label: t("byt.chart.call"), value: t(`byt.chart.calls.${r.call}`), info: use ?? undefined },
              { label: t("byt.chart.confidence"), value: r.confidence ? confidenceText(t, r.confidence) : NO_VALUE },
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
              { label: t("byt.chart.support"), value: r.support.length ? <Values items={r.support.map((v) => formatPrice(v, locale))} /> : NO_VALUE },
              { label: t("byt.chart.resistance"), value: r.resistance.length ? <Values items={r.resistance.map((v) => formatPrice(v, locale))} /> : NO_VALUE },
              { label: t("byt.chart.invalidation"), value: r.invalidation_level === null ? NO_VALUE : formatPrice(r.invalidation_level, locale) },
              { label: t("byt.chart.model"), value: <ModelId model={r.model} prompt={r.prompt_version} />, tone: "muted" },
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
            <p className="ae-byt__reason" data-tone={r.bar.status === "met" ? undefined : "warn"} data-testid="byt-bar">
              {bar} {use !== bar ? use : null}
            </p>
          ) : null}
        </>
      )}
      {r.latest_attempt_status && r.latest_attempt_status !== "ok" && r.latest_attempt_at ? (
        <p className="ae-subtle">
          {t("byt.chart.latestAttempt", { time: formatTableTime(r.latest_attempt_at, locale, now), status: attemptStatusText(t, r.latest_attempt_status) })}
        </p>
      ) : null}
    </Row>
  );
}

/** A model id may break after its slashes only ("deepseek/<wbr>deepseek-v4…"). */
function ModelId({ model, prompt }: { model: string; prompt: string }) {
  const parts = model.split("/");
  return (
    <>
      {parts.map((p, i) => (
        <Fragment key={i}>
          <span className="ae-byt__nowrap">{i < parts.length - 1 ? `${p}/` : p}</span>
          {i < parts.length - 1 ? <wbr /> : null}
        </Fragment>
      ))}
      {prompt ? <span className="ae-byt__nowrap">{` · ${prompt}`}</span> : null}
    </>
  );
}

/** Several values that break only between items, never inside one. */
function Values({ items, sep = ", " }: { items: string[]; sep?: string }) {
  return (
    <>
      {items.map((v, i) => (
        <span key={`${v}:${i}`} className="ae-byt__nowrap">
          {v}
          {i < items.length - 1 ? sep : ""}
        </span>
      ))}
    </>
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
            info: sayText(t, b.trend_rule_code, b.trend_rule) ?? undefined,
          },
          {
            label: t("byt.market.volatility"),
            value: b.volatility_regime ? t(`byt.market.volStates.${b.volatility_regime}`) : NO_VALUE,
            info: sayText(t, b.volatility_rule_code, b.volatility_rule) ?? undefined,
          },
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
            info: g ? (sayText(t, g.rule_code, g.rule) ?? undefined) : undefined,
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
    { id: "next", header: t("byt.funding.next"), numeric: true, priority: 2, cell: (r) => formatTableTime(r.next_funding, locale, now) },
  ];
  return (
    <Panel title={t("byt.market.title")} aside={<LeafHead leaf={b} now={now} />} className="ae-byt__block">
      <Row title={t("byt.market.regime")} now={now} testId="byt-regime">
        {b.status === "ok" ? <FactList rows={regimeRows} /> : <Reason leaf={b} />}
      </Row>
      <ChartReadBlock r={m.chart_read} now={now} />
      <Row title={t("byt.vol.title")} leaf={atr} now={now} testId="byt-vol">
        <FactList
          rows={[
            range.status === "ok"
              ? {
                  label: t("byt.vol.range24h"),
                  value: t("byt.vol.rangeValue", {
                    low: range.low === null ? NO_VALUE : formatPrice(range.low, locale),
                    high: range.high === null ? NO_VALUE : formatPrice(range.high, locale),
                    pct: pct(range.range_pct, locale),
                  }),
                }
              : { label: t("byt.vol.range24h"), value: reasonText(t, range, locale) ?? t("byt.noData"), tone: "muted" },
            atr.status === "ok"
              ? {
                  label: t("byt.vol.atr"),
                  value: t("byt.vol.atrValue", { usd: num(atr.atr_usd, locale, 0), pct: pct(atr.atr_pct, locale), n: atr.bars_used }),
                  info: sayText(t, atr.method_code, atr.method) ?? undefined,
                }
              : { label: t("byt.vol.atr"), value: reasonText(t, atr, locale) ?? t("byt.noData"), tone: "muted" },
          ]}
        />
      </Row>
      <Row title={t("byt.funding.title")} leaf={f} now={now} testId="byt-funding">
        {f.status === "ok" ? (
          <>
            <DataTable label={t("byt.funding.title")} columns={fundingCols} rows={f.rows} rowKey={(r) => r.symbol} bare empty={<p className="ae-subtle">{t("byt.noData")}</p>} />
            {f.datahub_stale ? <p className="ae-byt__reason" data-tone="warn">{t("byt.funding.stale")}</p> : null}
            <Reason leaf={f} />
          </>
        ) : (
          <Reason leaf={f} />
        )}
      </Row>
    </Panel>
  );
}

// ---------------------------------------------------------------- Bot models

function templateName(t: TFunction, presetId: string, fallback?: string): string {
  return t(`strategy.preset.names.${presetId}`, { defaultValue: fallback || presetId });
}

/**
 * The farm's signal bots (schema_minor 2): paper, simulated fills, in the
 * payload's fixed order, never ranked. Absent from an older payload.
 */
function FarmSignalBots({ bots, code }: { bots: FarmSignalBot[]; code?: string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const usdt = (v: number) => formatNumber(v, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const cols: DataColumn<FarmSignalBot>[] = [
    {
      id: "market",
      header: t("byt.farm.signal.market"),
      keep: true,
      cell: (b) => (
        <span className="ae-byt__tcell">
          {farmMarketText(t, b.market)}
          <span className="ae-subtle">{farmBotStateText(t, b.state)}</span>
        </span>
      ),
    },
    { id: "days", header: t("byt.farm.days"), numeric: true, priority: 3, cell: (b) => num(b.days_running, locale, 2) },
    {
      id: "trades",
      header: t("byt.farm.signal.trades"),
      headerTip: t("byt.farm.signal.tradesTip"),
      numeric: true,
      keep: true,
      cell: (b) => t("byt.farm.signal.tradesValue", { closed: b.trades_closed, open: b.trades_open }),
    },
    {
      id: "pnl",
      header: t("byt.farm.signal.pnl"),
      headerTip: t("byt.farm.signal.pnlTip"),
      numeric: true,
      priority: 2,
      tone: (b) => (b.realized_net_pnl_usdt > 0 ? "up" : b.realized_net_pnl_usdt < 0 ? "down" : undefined),
      cell: (b) => usdt(b.realized_net_pnl_usdt),
    },
    {
      id: "ret",
      header: t("byt.farm.signal.ret"),
      headerTip: t("byt.farm.signal.retTip"),
      numeric: true,
      keep: true,
      tone: (b) => (b.return_pct > 0 ? "up" : b.return_pct < 0 ? "down" : undefined),
      cell: (b) => spct(b.return_pct, locale, 3),
    },
    { id: "capital", header: t("byt.farm.signal.capital"), numeric: true, priority: 3, cell: (b) => usdt(b.allocated_usdt) },
    { id: "dd", header: t("byt.farm.signal.dd"), numeric: true, priority: 2, cell: (b) => spct(b.max_drawdown_pct, locale, 3) },
    {
      id: "worst",
      header: t("byt.farm.signal.worst"),
      headerTip: t("byt.farm.signal.worstTip"),
      numeric: true,
      priority: 3,
      cell: (b) => spct(b.worst_trade_pct, locale, 3),
    },
    {
      id: "refusals",
      header: t("byt.farm.signal.refusals"),
      priority: 3,
      wrap: true,
      cell: (b) =>
        !b.refusals_reported
          ? t("byt.farm.signal.refusalsNotReported")
          : b.refusals.length
            ? b.refusals.map((r) => refusalText(t, r.code, r.n)).join(t("byt.text.listJoin"))
            : t("byt.farm.signal.refusalsNone"),
    },
  ];
  return (
    <div data-testid="byt-farm-signal">
      <div className="ae-byt__subhead">
        <h3>{t("byt.farm.signal.title")}</h3>
        <Chip tone="neutral">{t("byt.farm.paperSimulated")}</Chip>
      </div>
      <DataTable label={t("byt.farm.signal.title")} columns={cols} rows={bots} rowKey={(b) => b.id} empty={<p className="ae-subtle">{t("byt.noData")}</p>} />
      {code ? <p className="ae-subtle">{sayText(t, code, null)}</p> : null}
    </div>
  );
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
  const rankReasons = farmRankReasons(ranking);
  const rows = templatesInOrder(p);

  const cols: DataColumn<LabTemplate>[] = [
    {
      id: "name",
      header: t("byt.models.template"),
      keep: true,
      cell: (r) => (
        <span className="ae-byt__tcell">
          <span className="ae-subtle tabular ae-byt__tid">{r.template_id}</span>
          <button type="button" className="ae-link ae-byt__linkbtn ae-byt__tname" title={t("byt.models.open")} onClick={() => navigate(templateFormPath(r))}>
            {templateName(t, r.preset_id, farmById.get(r.template_id)?.name)}
          </button>
          <Chip tone={isProven(r) ? "success" : "warn"}>{t(isProven(r) ? "byt.models.proven" : "byt.models.experimental")}</Chip>
        </span>
      ),
    },
    {
      id: "test",
      header: t("byt.models.test"),
      headerTip: t("byt.models.testTip"),
      numeric: true,
      keep: true,
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
    {
      id: "universe",
      header: t("byt.models.universe"),
      priority: 3,
      cell: (r) => (r.universe.rule === "btcOnly" ? t("byt.models.btcOnly") : t("byt.models.topN", { from: r.universe.rank_from, to: r.universe.rank_from + r.universe.top_n - 1 })),
    },
    { id: "dd", header: t("byt.models.worstDd"), numeric: true, priority: 3, cell: (r) => spct(r.test_worst_bot_drawdown_pct, locale, 1) },
    {
      id: "ghost",
      header: t("byt.models.ghost"),
      headerTip: t("byt.models.ghostTip"),
      numeric: true,
      priority: 3,
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

  const ghostMethod = sayText(t, ghost.method_code, ghost.method);
  const farmText = farm.labels ? sayText(t, farm.labels.text_code, farm.labels.text) : null;
  const farmFunding = farm.labels ? sayText(t, farm.labels.funding_code, farm.labels.funding) : null;
  return (
    <Panel title={t("byt.models.title")} aside={<LeafHead leaf={p.lab} now={now} />} className="ae-byt__block">
      <div className="ae-byt__row">
        <p className="ae-byt__lead" data-testid="byt-claim-rule">{claimRuleText(t, p)}</p>
        <p className="ae-subtle">{t("byt.models.openHint")}</p>
        {p.lab.status === "ok" ? (
          <DataTable
            label={t("byt.models.title")}
            columns={cols}
            rows={rows}
            rowKey={(r) => r.template_id}
            rowNote={(r) => (r.template_id === p.lab.majors_caveat.template_id ? majorsCaveatText(t, p, locale) : null)}
          />
        ) : (
          <Reason leaf={p.lab} />
        )}
        <p className="ae-subtle">
          {sayText(t, p.order_code, p.order)} {recheckText(t, p, locale)}
        </p>
      </div>
      <Row title={t("byt.ghost.title")} leaf={ghost} now={now} testId="byt-ghost">
        <p>
          {ghost.days_running === null
            ? t("byt.noData")
            : t("byt.ghost.measuring", { days: num(ghost.days_running, locale, 1), min: ghost.min_days_for_ranking })}
        </p>
        <Reason leaf={ghost} />
        <p className="ae-subtle">
          {t("byt.ghost.notRanked")} {ghostMethod}
        </p>
      </Row>
      <Row title={t("byt.farm.title")} leaf={farm} now={now} testId="byt-farm">
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
            {farmText || farmFunding ? (
              <p className="ae-subtle">
                {farmText} {farmFunding}
              </p>
            ) : null}
            {farm.signal_bots ? <FarmSignalBots bots={farm.signal_bots} code={farm.signal_bots_code} /> : null}
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
                {rankReasons.length ? (
                  <ul>
                    {rankReasons.map((r, i) => (
                      <li key={`${r.code}:${i}`}>{sayCoded(t, r, locale, now)}</li>
                    ))}
                  </ul>
                ) : null}
              </div>
            )}
          </>
        )}
      </Row>
      <Row title={t("byt.similar.title")} leaf={p.similar_days} now={now} testId="byt-similar">
        <Reason leaf={p.similar_days} />
      </Row>
    </Panel>
  );
}

// ---------------------------------------------------------------- News

function ImpactRow({ type }: { type: ImpactType }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const label = impactTypeName(t, type.type);
  if (type.status === "unavailable") return <li>{t("byt.news.impactUnavailable", { label, reason: reasonText(t, type, locale) ?? t("byt.noData") })}</li>;
  const l = impactLine(type.horizons["24h"]);
  if (l.kind === "measuring") return <li>{t("byt.news.impactMeasuring", { label, n: l.n, needed: l.needed ?? 30 })}</li>;
  if (l.kind === "none") return <li>{t("byt.news.impactNone", { label })}</li>;
  return (
    <li>
      {t("byt.news.impactMeasured", { label, median: spct(l.median, locale), up: pct(l.shareUp, locale, 1), n: l.n, base: pct(l.baseShareUp, locale, 1) })}{" "}
      {l.differs && l.ci ? t("byt.news.differs", { lo: spct(l.ci[0], locale), hi: spct(l.ci[1], locale) }) : t("byt.news.noDifference")}
    </li>
  );
}

/**
 * One digest line: the server's checked translation for the reader's
 * language with a small "translation" label whose hover (and expander) holds
 * the English original; otherwise the English line, marked lang="en".
 */
function NewsText({ line, lang }: { line: NewsLine; lang: string }) {
  const { t } = useTranslation();
  if (!line.text) return <>{t("byt.news.withheldText")}</>;
  const tr = newsTranslation(line, lang);
  if (!tr) return <span lang="en">{line.text}</span>;
  return (
    <span className="ae-byt__tr" data-testid="byt-news-translated">
      <span lang={lang}>{tr}</span>{" "}
      <details className="ae-byt__orig">
        <summary className="ae-byt__trlabel" title={line.text} aria-label={t("byt.news.showOriginal")}>
          {t("byt.news.translated")}
        </summary>
        <span lang="en">{line.text}</span>
      </details>
    </span>
  );
}

function NewsBlock({ v, now }: { v: BeforeYouTradeView; now: number }) {
  const { t, i18n } = useTranslation();
  const lang = i18n.resolvedLanguage ?? "en";
  const locale = localeForLanguage(lang);
  const n = v.payload.news;
  const translatedNote = sayText(t, n.translation_code ?? "news_machine_translated_v1", null);
  const shownTranslated = n.lines.filter((l) => newsTranslation(l, lang) !== null).length;
  // The source-language note stays while any shown line is still in English.
  const anyEnglish = n.lines.some((l) => l.text && newsTranslation(l, lang) === null);
  const types = n.impact.types ?? [];
  const lineCols: DataColumn<NewsLine>[] = [
    {
      id: "text",
      header: t("byt.news.line"),
      headerTip: shownTranslated ? translatedNote : t("byt.news.sourceLanguage"),
      wrap: true,
      keep: true,
      // Third-party reporting: the server's checked translation, or the English line marked for screen readers.
      cell: (l) => <NewsText line={l} lang={lang} />,
    },
    {
      id: "source",
      header: t("byt.news.source"),
      priority: 2,
      cell: (l) => <SourceLink url={l.url}>{l.source_name || t("byt.news.sourceLink")}</SourceLink>,
    },
    { id: "time", header: t("table.time"), numeric: true, priority: 3, width: "1%", cell: (l) => (l.published_at ? formatTableTime(l.published_at, locale, now) : NO_VALUE) },
  ];
  return (
    <Panel title={t("byt.news.title")} aside={<LeafHead leaf={n} now={now} />} className="ae-byt__block">
      <div className="ae-byt__row">
        {n.newer_run_code ? (
          <p className="ae-byt__reason" data-testid="byt-news-newer-run">
            {sayText(t, n.newer_run_code, null, { time: n.newer_run_at ? formatTableTime(n.newer_run_at, locale, now) : NO_VALUE })}
          </p>
        ) : null}
        {n.status === "ok" || n.lines.length ? (
          <DataTable label={t("byt.news.title")} columns={lineCols} rows={n.lines} rowKey={(l) => l.url || l.text} empty={<p className="ae-subtle">{t("byt.noData")}</p>} />
        ) : (
          <Reason leaf={n} />
        )}
        <p className="ae-subtle">
          {t("byt.news.count", { shown: n.lines.length, total: n.lines_total })}
          {n.withheld_lines > 0 ? ` ${t("byt.news.withheld", { count: n.withheld_lines })}` : ""} {shownTranslated ? `${translatedNote} ` : ""}
          {anyEnglish || !shownTranslated ? `${t("byt.news.sourceLanguage")} ` : ""}
          {sayText(t, n.disclosure_code, n.disclosure)}
        </p>
      </div>
      <Row title={t("byt.news.impactTitle", { days: n.impact.window_days })} leaf={n.impact} now={now} testId="byt-impact">
        {n.impact.status === "ok" || types.length ? (
          <ul className="ae-byt__impact">
            {types.map((ty) => (
              <ImpactRow key={ty.type} type={ty} />
            ))}
          </ul>
        ) : null}
        <Reason leaf={n.impact} />
        <p className="ae-subtle">{sayText(t, n.impact.disclosure_code, n.impact.disclosure)}</p>
      </Row>
    </Panel>
  );
}

// ---------------------------------------------------------------- Listings

function ReactionText({ row }: { row: ListingRow }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const c = reactionCell(row);
  const why = row.reaction ? reasonText(t, row.reaction, locale) : null;
  if (c.kind === "noData")
    return (
      <span className="ae-subtle">
        {t("byt.noData")}
        {why ? ` · ${why}` : ""}
      </span>
    );
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
      // The exchange's own announcement: linked, its title (in the exchange's language) on hover.
      cell: (r) => (
        <SourceLink url={r.announcement.url} hint={r.announcement.title || undefined}>
          {t("byt.listings.announcement", { exchange: r.exchange || r.announcement.source })}
        </SourceLink>
      ),
    },
    { id: "reaction", header: t("byt.listings.reaction"), headerTip: sayText(t, l.reaction_method_code, l.reaction_method) ?? undefined, cell: (r) => <ReactionText row={r} />, wrap: true },
  ];
  const summary = l.reaction_summary ?? [];
  const rowKey = (r: ListingRow) => `${r.exchange}:${r.market}:${r.symbol}:${r.listing_time}`;
  return (
    <Panel title={t("byt.listings.title")} aside={<LeafHead leaf={l} now={now} />} className="ae-byt__block">
      {l.reason || l.reason_code ? <Reason leaf={{ ...l, status: "unavailable" }} /> : null}
      {l.status === "unavailable" && !l.upcoming.length && !l.recent.length ? null : (
        <>
          <Row title={t("byt.listings.upcoming", { shown: l.upcoming.length, total: l.upcoming_total })} now={now}>
            <DataTable label={t("byt.listings.upcomingLabel")} columns={cols} rows={l.upcoming} rowKey={rowKey} empty={<p className="ae-subtle">{t("byt.listings.none")}</p>} />
          </Row>
          <Row title={t("byt.listings.recent", { shown: l.recent.length, total: l.recent_total })} now={now}>
            <DataTable label={t("byt.listings.recentLabel")} columns={cols} rows={l.recent} rowKey={rowKey} empty={<p className="ae-subtle">{t("byt.listings.none")}</p>} />
          </Row>
        </>
      )}
      {summary.length ? (
        <Row title={t("byt.listings.summary")} now={now}>
          <ul className="ae-byt__impact">
            {summary.map((s) => (
              <li key={`${s.exchange}:${s.market}`}>
                {t("byt.listings.summaryLine", {
                  exchange: s.exchange,
                  market: t(`byt.listings.markets.${s.market}`, { defaultValue: s.market }),
                  listings: s.listings,
                  noData: s.no_data,
                  pending: s.pending,
                })}{" "}
                {s.horizons.some((h) => h.n > 0)
                  ? s.horizons.map((h) => t("byt.listings.horizon", { h: h.horizon, median: spct(h.median_pct, locale), up: pct(h.positive_rate_pct, locale, 0), n: h.n })).join(" · ")
                  : t("byt.noData")}
              </li>
            ))}
          </ul>
        </Row>
      ) : null}
      <p className="ae-subtle">
        {t("byt.listings.sources", {
          list: l.sources.map((s) => (s.status === "ok" ? s.id : `${s.id} (${feedStatusText(t, s.status)})`)).join(", "),
        })}{" "}
        {sayText(t, l.disclosure_code, l.disclosure)}
      </p>
    </Panel>
  );
}

// ---------------------------------------------------------------- Page

export function BeforeYouTradePage() {
  const { t, i18n } = useTranslation();
  const lang = i18n.resolvedLanguage ?? "en";
  const locale = localeForLanguage(lang);
  const now = useNowTick();
  const [view, setView] = useState<BeforeYouTradeView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(
    (refresh: boolean) => {
      setLoading(true);
      // The UI language: the server sends that locale's translations; a
      // language change loads that locale's copy.
      beforeYouTrade(refresh, lang)
        .then((v) => {
          setView(v);
          setError(null);
        })
        .catch((e: unknown) => setError(errorMessage(e, "bytUnavailable")))
        .finally(() => setLoading(false));
    },
    [lang],
  );
  useEffect(() => load(false), [load]);

  const errorText = (code: string) => {
    const [head, detail] = code.split("|");
    return t(`byt.errors.${head}`, { defaultValue: t("byt.errors.unknown"), detail });
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
      <ul className="ae-byt__facts" data-testid="byt-about">
        {disclosureFacts(t, p, locale).map((f) => (
          <li key={f}>{f}</li>
        ))}
      </ul>
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
