import { useEffect, useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { Link, navigate, useRoute } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { Button } from "@/components/ui/Button/Button";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FilterGroup } from "@/components/ui/FilterPanel/FilterPanel";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPercent, formatSignedPercent, NO_VALUE, pnlToneAttr } from "@/lib/format";
import { errorMessage } from "@/lib/ipc/bridge";
import { liveRankable, presetsLive, type LiveTemplate, type PresetsLive } from "@/lib/ipc/strategy/presetsLive";
import { strategyPresets, type Preset, type StrategyKind } from "@/lib/ipc/strategy/strategy";
import { presetName, sideText, strategyErrorText, universeText } from "@/lib/strategyText";
import { monthName, reasonLines, shortDate, testSplit, useTemplateLauncher, VerdictChip } from "./presetUi";
import { InfoTip } from "@/components/ui/InfoTip/InfoTip";
import { MetaLine } from "@/components/ui/MetaLine/MetaLine";
import "./Presets.css";

const KINDS: StrategyKind[] = ["dca", "grid"];

function usePresets() {
  const { t } = useTranslation();
  const [presets, setPresets] = useState<Preset[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    strategyPresets()
      .then((p) => alive && setPresets(p))
      .catch((e: unknown) => alive && setError(strategyErrorText(t, errorMessage(e, "storeReadFailed"))));
    return () => {
      alive = false;
    };
  }, [t]);
  return { presets, error };
}

/** The server's ghost track (null while loading or when unavailable). */
function usePresetsLive() {
  const [live, setLive] = useState<PresetsLive | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let alive = true;
    presetsLive()
      .then((v) => alive && setLive(v))
      .catch(() => alive && setFailed(true));
    return () => {
      alive = false;
    };
  }, []);
  return { live, failed };
}

function liveOf(live: PresetsLive | null, id: string): LiveTemplate | null {
  return live?.templates.find((x) => x.id === id) ?? null;
}

/** "8 Oct" (UTC day), or with the local time for an update stamp. */
function liveDate(iso: string, locale: string, time = false): string {
  return new Date(iso).toLocaleString(locale, time ? { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" } : { day: "numeric", month: "short", timeZone: "UTC" });
}

/** Ghost track status line: start, day count against the 30-day rule, last update. */
function LiveLine({ live, failed }: { live: PresetsLive | null; failed: boolean }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  if (failed) return <p className="ae-subtle">{t("strategy.preset.live.unavailable")}</p>;
  if (!live) return null;
  const date = (iso: string, time = false) => liveDate(iso, locale, time);
  return (
    <p className="ae-subtle">
      {t(liveRankable(live) ? "strategy.preset.live.lineReady" : "strategy.preset.live.line", {
        since: date(live.t0),
        days: Math.floor(live.days),
        min: live.minDaysForRanking,
        updated: date(live.updatedAt, true),
      })}
    </p>
  );
}

/** One template's ghost track on its own page: always the numbers, with the 30-day note while early. */
function LivePanel({ live, failed, id }: { live: PresetsLive | null; failed: boolean; id: string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const l = liveOf(live, id);
  return (
    <Panel title={t("strategy.preset.live.title")}>
      {failed ? (
        <p className="ae-subtle">{t("strategy.preset.live.unavailable")}</p>
      ) : !live || !l ? (
        <p className="ae-subtle">{t("workspace.loading")}</p>
      ) : (
        <>
          {!liveRankable(live) ? (
            <p className="ae-banner" data-tone="info">
              {t("strategy.preset.live.early", { days: Math.floor(live.days), min: live.minDaysForRanking })}
            </p>
          ) : null}
          <FactList
            rows={[
              // A total since the start, not a per-month figure like the test mean.
              { label: t("strategy.preset.live.return", { since: liveDate(live.t0, locale) }), value: formatSignedPercent(l.returnPct, locale), tone: l.returnPct >= 0 ? "up" : "down" },
              { label: t("strategy.preset.worstBotDd"), value: l.worstDdPct === null ? NO_VALUE : formatSignedPercent(l.worstDdPct, locale, 1) },
              { label: t("strategy.preset.live.medianDd"), value: l.medianDdPct === null ? NO_VALUE : formatSignedPercent(l.medianDdPct, locale, 1) },
              { label: t("strategy.preset.live.bots"), value: l.bots },
              { label: t("strategy.preset.live.cycles"), value: `${l.cycles} · ${t("strategy.preset.live.open", { count: l.openCycles })}` },
            ]}
          />
          <LiveLine live={live} failed={false} />
          <p className="ae-subtle">{t("strategy.preset.live.method")}</p>
        </>
      )}
    </Panel>
  );
}

/** Settings of a preset in one line of labels and numbers. */
function configFacts(t: TFunction, p: Preset, locale: string): Fact[] {
  const c = p.config;
  const n = (v: number) => formatNumber(v, locale, { maximumFractionDigits: 2 });
  const rows: Fact[] = [
    { label: t("table.type"), value: t(`botsList.type.${c.params.kind}`) },
    { label: t("table.market"), value: t(`filters.market.${c.market}`) },
    { label: t("table.side"), value: sideText(t, c.side) },
    { label: t("table.leverage"), value: `${c.leverage}x` },
  ];
  if (c.params.kind === "dca") {
    const d = c.params;
    rows.push(
      {
        label: t("strategy.field.sizing"),
        value: d.baseWeight !== null ? `${t("strategy.sizing.weights")} ${n(d.baseWeight)} / ${n(d.safetyWeight ?? 1)}` : t("strategy.sizing.fixed"),
      },
      { label: t("strategy.field.maxSo"), value: d.maxSo },
      { label: t("strategy.field.soStep"), value: `${n(d.soStepPct)}%` },
      { label: t("strategy.field.stepScale"), value: n(d.stepScale) },
      { label: t("strategy.field.volumeScale"), value: n(d.volumeScale) },
      { label: t("strategy.field.tp"), value: `${n(d.tpPct)}%` },
      { label: t("strategy.field.sl"), value: d.slPct !== null ? `${n(d.slPct)}%` : t("strategy.preset.none"), tone: d.slPct === null ? "warn" : undefined },
    );
  } else {
    const g = c.params;
    rows.push(
      { label: t("strategy.field.nGrids"), value: g.nGrids },
      { label: t("strategy.field.spacing"), value: t(`strategy.spacing.${g.spacing}`) },
      { label: t("strategy.field.stopOutPct"), value: g.stopOutPct !== null ? `${n(g.stopOutPct)}%` : t("strategy.preset.none") },
    );
  }
  rows.push(
    { label: t("strategy.field.ddStop"), value: c.maxDrawdownPct !== null ? `${n(c.maxDrawdownPct)}%` : t("strategy.preset.none"), tone: c.maxDrawdownPct === null ? "warn" : undefined },
    { label: t("strategy.field.btcGate"), value: t(c.pauseOnBtcBreak ? "strategy.preset.on" : "strategy.preset.off") },
    { label: t("strategy.risk.breaker"), value: t(c.portfolioBreaker ? "strategy.preset.on" : "strategy.preset.off") },
  );
  return rows;
}

/**
 * #/presets. Only presets the engine ships (strategy_presets: verdict
 * PRESET-READY), each with its historical-simulation numbers as served.
 * A simulation is labelled as one, never as a track record.
 */
export function PresetsPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const route = useRoute();
  const { presets, error } = usePresets();
  const type = route.query.get("type");
  const filter = type === "dca" || type === "grid" ? type : null;
  const rows = presets?.filter((p) => !filter || p.config.params.kind === filter) ?? null;
  const { launch, dialog } = useTemplateLauncher();
  const { live, failed: liveFailed } = usePresetsLive();

  const left = (
    <FilterGroup
      label={t("filters.type")}
      single
      options={KINDS.map((k) => ({ value: k, label: t(`botsList.type.${k}`), count: presets?.filter((p) => p.config.params.kind === k).length ?? 0 }))}
      selected={filter ? [filter] : []}
      onChange={(v) => navigate(withQuery("/presets", { type: v[0] ?? null }), { replace: true })}
    />
  );

  const columns: DataColumn<Preset>[] = [
    {
      id: "name",
      header: t("table.name"),
      cell: (p) => (
        <>
          <Link to={`/presets/${p.id}`} className="ae-link">
            {presetName(t, p)}
          </Link>
          <span className="ae-preset__situation">{t(`strategy.preset.situation.${p.situation}`, { defaultValue: p.situation })}</span>
        </>
      ),
    },
    { id: "type", header: t("table.type"), priority: 3, cell: (p) => t(`botsList.type.${p.config.params.kind}`) },
    { id: "market", header: t("table.market"), priority: 3, cell: (p) => t(`filters.market.${p.config.market}`) },
    { id: "side", header: t("table.side"), priority: 2, cell: (p) => `${sideText(t, p.config.side)} · ${p.config.leverage}x` },
    {
      id: "testMean",
      header: t("strategy.preset.col.testMean"),
      headerTip: t("strategy.preset.testMean"),
      detailLabel: t("strategy.preset.testMean"),
      numeric: true,
      cell: (p) => {
        const s = testSplit(p);
        return s ? formatSignedPercent(s.meanPerBotMonthPct, locale) : NO_VALUE;
      },
      tone: (p) => pnlToneAttr(testSplit(p)?.meanPerBotMonthPct),
    },
    {
      id: "ci95",
      header: t("strategy.preset.ci95"),
      numeric: true,
      priority: 3,
      cell: (p) => {
        const s = testSplit(p);
        return s ? `${formatNumber(s.ci95LowPct, locale, { maximumFractionDigits: 2 })} – ${formatPercent(s.ci95HighPct, locale)}` : NO_VALUE;
      },
    },
    {
      id: "months",
      header: t("strategy.preset.monthsPositive"),
      numeric: true,
      priority: 2,
      cell: (p) => {
        const s = testSplit(p);
        return s ? `${s.monthsPositive}/${s.months}` : NO_VALUE;
      },
    },
    {
      id: "worstDd",
      header: t("strategy.col.drawdownShort"),
      headerTip: t("strategy.preset.worstBotDd"),
      detailLabel: t("strategy.preset.worstBotDd"),
      numeric: true,
      priority: 2,
      cell: (p) => formatSignedPercent(p.history.testWorstBotDrawdownPct, locale, 1),
    },
    {
      // A total since the ghost start, not per month: the header says since when.
      id: "live",
      header: live ? t("strategy.preset.live.col", { since: liveDate(live.t0, locale) }) : t("strategy.preset.live.title"),
      headerTip: t("strategy.preset.live.tip"),
      detailLabel: live ? t("strategy.preset.live.col", { since: liveDate(live.t0, locale) }) : t("strategy.preset.live.title"),
      numeric: true,
      cell: (p) => {
        const l = liveOf(live, p.id);
        if (!live || !l) return NO_VALUE;
        if (!liveRankable(live)) {
          return <span className="ae-subtle">{t("strategy.preset.live.collecting", { days: Math.floor(live.days), min: live.minDaysForRanking })}</span>;
        }
        return formatSignedPercent(l.returnPct, locale);
      },
      tone: (p) => (live && liveRankable(live) ? pnlToneAttr(liveOf(live, p.id)?.returnPct) : undefined),
    },
    {
      id: "verdict",
      header: t("strategy.preset.verdict"),
      // The verdict is the column a beginner needs most: never folded away.
      keep: true,
      cell: (p) => <VerdictChip p={p} />,
    },
    {
      id: "reason",
      header: t("strategy.preset.reasonCol"),
      detailOnly: true,
      wrap: true,
      cell: (p) =>
        p.failReasons.length
          ? reasonLines(t, p)
          : p.history.recheck
            ? t("strategy.preset.checksToWindow", { date: shortDate(p.history.recheck.testWindowEnd, locale) })
            : t("strategy.preset.allChecksPassed"),
    },
  ];

  return (
    <PageShell title={t("nav.presets")} crumbs={[{ label: t("nav.groups.research") }]} left={left}>
      <p>
        <Link to="/guide?s=which-bot" className="ae-link">
          {t("guide.presetsLink")}
        </Link>
      </p>
      <MetaLine
        items={[{ text: t("backtest.label.historical") }]}
        aside={
          <InfoTip
            label={t("backtest.label.historical")}
            content={
              <>
                {t("strategy.preset.simulationLabel")}
                <br />
                {t("strategy.preset.testMeanNote")}
              </>
            }
          />
        }
      />
      <LiveLine live={live} failed={liveFailed} />
      {error ? (
        <EmptyState tone="error" title={error} />
      ) : (
        <DataTable
          label={t("nav.presets")}
          columns={columns}
          rows={rows ?? []}
          rowKey={(p) => p.id}
          loading={rows === null}
          empty={<EmptyState title={filter ? t(`strategy.preset.noneOf.${filter}`) : t("strategy.preset.empty")} />}
          actions={(p) => (
            <>
              <Button variant="secondary" size="xs" onClick={() => launch(p)}>
                {t("strategy.preset.use")}
              </Button>
              <Button variant="ghost" size="xs" onClick={() => navigate(`/backtest?preset=${p.id}`)}>
                {t("backtest.backtestThis")}
              </Button>
            </>
          )}
        />
      )}
      {dialog}
    </PageShell>
  );
}

/** #/presets/:presetId — one preset, every number of its simulation. */
export function PresetDetailPage({ presetId }: { presetId: string }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { presets, error } = usePresets();
  const { live, failed: liveFailed } = usePresetsLive();
  const crumbs = [
    { label: t("nav.groups.research") },
    { label: t("nav.presets"), to: "/presets" },
  ];
  const p = presets?.find((x) => x.id === presetId) ?? null;
  const { launch, dialog } = useTemplateLauncher();

  if (error || (presets && !p)) {
    return (
      <PageShell title={t("page.presetDetail")} crumbs={crumbs} single>
        <EmptyState
          tone={error ? "error" : "default"}
          title={error ?? t("strategy.form.presetUnknown", { id: presetId })}
          actions={
            <Button variant="secondary" size="sm" onClick={() => navigate("/presets")}>
              {t("nav.presets")}
            </Button>
          }
        />
      </PageShell>
    );
  }
  if (!p) {
    return (
      <PageShell title={t("page.presetDetail")} crumbs={crumbs} single>
        <p className="ae-subtle">{t("workspace.loading")}</p>
      </PageShell>
    );
  }

  const h = p.history;
  const pctN = (v: number, d = 2) => `${formatNumber(v, locale, { maximumFractionDigits: d })}%`;

  const opt = (v: number | null, f: (n: number) => string) => (v === null ? NO_VALUE : f(v));
  const hasPerDay = h.splits.some((s) => s.oneBotReturnPerDayPct !== null);

  return (
    <PageShell
      title={presetName(t, p)}
      crumbs={crumbs}
      secondary={
        <Button variant="secondary" size="sm" onClick={() => navigate(`/backtest?preset=${p.id}`)}>
          {t("backtest.backtestThis")}
        </Button>
      }
      primary={
        <Button size="sm" onClick={() => launch(p)}>
          {t("strategy.preset.use")}
        </Button>
      }
      left={
        <Panel title={t("strategy.preset.settings")}>
          <FactList rows={configFacts(t, p, locale)} />
        </Panel>
      }
      right={
        <>
          <Panel title={t("strategy.preset.universeTitle")}>
            <FactList
              rows={[
                { label: t("strategy.preset.rule"), value: universeText(t, p.universe) },
                { label: t("strategy.preset.reselect"), value: t(`strategy.preset.reselectEvery.${p.universe.reselect}`, { defaultValue: p.universe.reselect }) },
                { label: t("strategy.preset.excluded"), value: p.universe.exclude.join(", ") },
                { label: t("strategy.preset.capitalModel"), value: t(`strategy.preset.capital.${p.capitalModel}`, { defaultValue: p.capitalModel }) },
              ]}
            />
          </Panel>
          <Panel title={t("strategy.preset.method")}>
            <FactList
              rows={[
                { label: t("strategy.preset.simulator"), value: `${h.simulator} · ${h.barInterval}` },
                { label: t("strategy.preview.makerFee"), value: pctN(h.makerFeePct, 3) },
                { label: t("strategy.preview.takerFee"), value: pctN(h.takerFeePct, 3) },
                { label: t("strategy.preview.slippage"), value: pctN(h.slippagePct, 3) },
                { label: t("strategy.preset.fillRule"), value: t(`strategy.preset.fillRules.${h.fillRule}`, { defaultValue: h.fillRule }) },
              ]}
            />
          </Panel>
        </>
      }
    >
      <MetaLine
        items={[{ text: t("backtest.label.historical") }]}
        aside={
          <InfoTip
            label={t("backtest.label.historical")}
            content={
              <>
                {t("strategy.preset.simulationLabel")}
                <br />
                {t("strategy.preset.testMeanNote")}
              </>
            }
          />
        }
      />
      <p className="ae-preset__situation-line">{t(`strategy.preset.situation.${p.situation}`, { defaultValue: p.situation })}</p>
      <LivePanel live={live} failed={liveFailed} id={p.id} />
      {p.verdict === "failed" ? (
        <div className="ae-banner" data-tone="danger" role="note">
          <strong>{t("strategy.preset.failedBanner")}</strong>
          {reasonLines(t, p)}
        </div>
      ) : (
        <p className="ae-banner" data-tone={h.recheck ? "warn" : "info"}>
          {h.recheck
            ? t("strategy.preset.recheckBanner", {
                windowEnd: shortDate(h.recheck.testWindowEnd, locale),
                month: monthName(h.recheck.partialMonth, locale),
                days: h.recheck.partialMonthDays,
                mean: h.recheck.partialMonthMeanPct.toFixed(2),
                nextRead: shortDate(h.recheck.nextRead, locale),
              })
            : t("strategy.preset.passedBanner")}
        </p>
      )}
      <Panel title={t("strategy.preset.splits")}>
        <DataTable
          label={t("strategy.preset.splits")}
          bare
          rows={h.splits}
          rowKey={(s) => s.split}
          columns={[
            { id: "split", header: t("strategy.preset.split"), keep: true, cell: (s) => t(`strategy.preset.splitName.${s.split}`) },
            {
              id: "mean",
              header: t("strategy.preset.meanPerBotMonth"),
              numeric: true,
              cell: (s) => formatSignedPercent(s.meanPerBotMonthPct, locale),
              tone: (s) => (s.meanPerBotMonthPct > 0 ? "up" : "down"),
            },
            {
              id: "ci",
              header: t("strategy.preset.ci95"),
              numeric: true,
              priority: 2,
              cell: (s) => `${formatNumber(s.ci95LowPct, locale, { maximumFractionDigits: 2 })} – ${pctN(s.ci95HighPct)}`,
            },
            { id: "months", header: t("strategy.preset.monthsPositive"), numeric: true, cell: (s) => `${s.monthsPositive}/${s.months}` },
            ...(hasPerDay
              ? [
                  {
                    id: "perDay",
                    header: t("strategy.preset.oneBotPerDay"),
                    numeric: true,
                    priority: 3 as const,
                    cell: (s: (typeof h.splits)[number]) => opt(s.oneBotReturnPerDayPct, (v) => formatSignedPercent(v, locale, 3)),
                  },
                ]
              : []),
          ]}
        />
      </Panel>
      <Panel title={t("strategy.preset.testFacts")}>
        <FactList
          rows={[
            { label: t("strategy.preset.testBots"), value: h.testBots },
            { label: t("strategy.preset.shareBotsPositive"), value: pctN(h.testShareBotsPositive * 100, 1) },
            { label: t("strategy.preset.worstBotDd"), value: formatSignedPercent(h.testWorstBotDrawdownPct, locale, 1), tone: "down" },
            ...(h.testMeanAtTaker010Pct !== null
              ? [{ label: t("strategy.preset.meanAtTaker010"), value: formatSignedPercent(h.testMeanAtTaker010Pct, locale) }]
              : []),
            { label: t("strategy.preset.signTest"), value: t("strategy.preset.signTestValue", { p: h.testSignTestP, bar: h.bonferroniBar }) },
            ...(h.oneBotWorstDrawdownPct !== null
              ? [{ label: t("strategy.preset.oneBotWorstDd"), value: formatSignedPercent(h.oneBotWorstDrawdownPct, locale, 1), tone: "down" as const }]
              : []),
            ...(h.oneBotMedianDrawdownPct !== null
              ? [{ label: t("strategy.preset.oneBotMedianDd"), value: formatSignedPercent(h.oneBotMedianDrawdownPct, locale, 1) }]
              : []),
            { label: t("strategy.preset.maxDealDays"), value: h.maxDealDays, tone: h.maxDealDays > 30 ? "warn" : undefined },
            { label: t("strategy.preset.openAtEnd"), value: h.openDealsAtDataEnd },
            { label: t("strategy.preset.liquidations"), value: h.liquidations, tone: h.liquidations > 0 ? "down" : undefined },
            { label: t("strategy.preset.stoppedDeals"), value: h.stoppedDeals },
          ]}
        />
      </Panel>
      <p className="ae-subtle">{t("strategy.preset.parityNote")}</p>
      {dialog}
    </PageShell>
  );
}
