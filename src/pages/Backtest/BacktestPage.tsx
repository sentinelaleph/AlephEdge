import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate, useRoute } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { ConfirmDialog } from "@/components/ui/ConfirmDialog/ConfirmDialog";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { KpiGrid } from "@/components/ui/KpiGrid/KpiGrid";
import { KpiTile } from "@/components/ui/KpiTile/KpiTile";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import i18n, { localeForLanguage } from "@/i18n";
import {
  formatNumber,
  formatPercent,
  formatPnl,
  formatPrice,
  formatSignedPercent,
  formatSignedUsdt,
  formatTableTime,
  formatUsdt,
  NO_VALUE,
  pnlToneAttr,
} from "@/lib/format";
import { errorMessage, type Unlisten } from "@/lib/ipc/bridge";
import {
  BACKTEST_INTERVALS,
  backtestDelete,
  backtestDeleteAll,
  backtestGet,
  backtestList,
  backtestRun,
  newBacktestRunToken,
  onBacktestProgress,
  type BacktestInterval,
  type BacktestRun,
  type BacktestRunSummary,
} from "@/lib/ipc/strategy/backtest";
import {
  strategyDefault,
  strategyPresets,
  type Preset,
  type StrategyConfig,
  type StrategyKind,
} from "@/lib/ipc/strategy/strategy";
import { sideText, strategyErrorText, strategyNoteText } from "@/lib/strategyText";
import { EquityChart } from "@/pages/Bots/strategy/EquityChart";
import { Segmented } from "@/pages/Bots/strategy/formControls";
import { matchesPreset, StrategyForm, type FormPreview } from "@/pages/Bots/strategy/StrategyForm";
import { LeverageRiskDialog, needsLiquidationConfirm } from "@/pages/Bots/strategy/LeverageRiskDialog";
import { StrategyPreviewPanel } from "@/pages/Bots/strategy/StrategyPreviewPanel";
import { runStore } from "./backtestRunStore";
import { MetaLine } from "@/components/ui/MetaLine/MetaLine";
import "./Backtest.css";

const DAY = 86_400_000;
const DEFAULT_SYMBOL = "BTCUSDT";
const WINDOWS = [
  { id: "30d", days: 30 },
  { id: "90d", days: 90 },
  { id: "180d", days: 180 },
  { id: "1y", days: 365 },
  { id: "2y", days: 730 },
  { id: "custom", days: 0 },
] as const;
type WindowId = (typeof WINDOWS)[number]["id"];

/** "code|detail" -> `backtest.errors.<code>`, else the strategy validation text. */
export function backtestErrorText(t: TFunction, raw: string): string {
  const [code, ...rest] = raw.split("|");
  const detail = rest.join("|").trim();
  const key = `backtest.errors.${code.trim()}`;
  if (!i18n.exists(key)) return strategyErrorText(t, raw);
  const text = t(key);
  return detail ? `${text} (${detail})` : text;
}

const utcDay = (ms: number) => new Date(ms).toISOString().slice(0, 10);
const parseDay = (s: string) => {
  const ms = Date.parse(`${s}T00:00:00Z`);
  return Number.isFinite(ms) ? ms : null;
};

function useLocale() {
  const { i18n } = useTranslation();
  return localeForLanguage(i18n.resolvedLanguage ?? "en");
}

function windowText(locale: string, start: number, end: number): string {
  const f = (ms: number) => new Date(ms).toLocaleDateString(locale, { timeZone: "UTC" });
  return `${f(start)} – ${f(end - 1)}`;
}

/**
 * #/backtest (?preset=<id> | ?run=<runId> | ?type=dca|grid). Left: the same
 * DCA / Grid form bot creation uses, plus interval and window; center: the
 * stored runs. Candles come from DataHub through the Sentinel API; the run
 * uses the paper engine's bar driver. Nothing here is a track record.
 */
export function BacktestPage() {
  const { t } = useTranslation();
  const locale = useLocale();
  const route = useRoute();
  const { membership } = useDeskContext();
  const presetId = route.query.get("preset");
  const fromRun = route.query.get("run");
  const typeParam = route.query.get("type");
  const signedIn = membership.view.authenticated;

  const [kind, setKind] = useState<StrategyKind>(typeParam === "grid" ? "grid" : "dca");
  const [interval, setBarInterval] = useState<BacktestInterval>("1h");
  const [windowId, setWindowId] = useState<WindowId>("90d");
  const [customStart, setCustomStart] = useState(() => utcDay(Date.now() - 90 * DAY));
  const [customEnd, setCustomEnd] = useState(() => utcDay(Date.now()));
  const [initial, setInitial] = useState<StrategyConfig | null>(null);
  const [preset, setPreset] = useState<Preset | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [formKey, setFormKey] = useState(0);
  const [form, setForm] = useState<{ cfg: StrategyConfig; ok: boolean } | null>(null);
  const [summary, setSummary] = useState<FormPreview | null>(null);
  // The run in flight is kept outside the page (see backtestRunStore).
  const { flight, error: runError, finished } = useSyncExternalStore(runStore.subscribe, runStore.get);
  const running = flight !== null;
  const progress = flight?.progress ?? null;
  useEffect(() => runStore.mount(), []);
  const [runs, setRuns] = useState<BacktestRunSummary[] | null>(null);
  const [deleting, setDeleting] = useState<{ id: string; name: string } | "all" | null>(null);
  const [deleteError, setDeleteError] = useState<string | null>(null);
  const [listError, setListError] = useState<string | null>(null);
  const kindRef = useRef(kind);
  kindRef.current = kind;

  // The starting config: a stored run's, a preset's, or Rust's defaults.
  useEffect(() => {
    let alive = true;
    setInitial(null);
    setLoadError(null);
    const load = async (): Promise<{ cfg: StrategyConfig; preset: Preset | null; kind: StrategyKind }> => {
      if (fromRun) {
        const r = await backtestGet(fromRun);
        if (alive) {
          setBarInterval(r.interval);
          setWindowId("custom");
          setCustomStart(utcDay(r.startMs));
          setCustomEnd(utcDay(r.endMs - 1));
        }
        return { cfg: r.config, preset: null, kind: r.config.params.kind };
      }
      if (presetId) {
        const p = (await strategyPresets()).find((x) => x.id === presetId);
        if (!p) throw t("strategy.form.presetUnknown", { id: presetId });
        // A template ranked below the top (alts: 6 to 15) was never tested
        // on BTCUSDT: the user picks a pair from its list.
        const symbol = p.config.symbol || (p.universe.rankFrom > 1 ? "" : DEFAULT_SYMBOL);
        return { cfg: { ...p.config, symbol, presetId: p.id }, preset: p, kind: p.config.params.kind };
      }
      const k = kindRef.current;
      const cfg = await strategyDefault(k, "binance", DEFAULT_SYMBOL);
      return { cfg: { ...cfg, name: `${k === "dca" ? "DCA" : "Grid"} ${DEFAULT_SYMBOL}` }, preset: null, kind: k };
    };
    load()
      .then((r) => {
        if (!alive) return;
        setKind(r.kind);
        setInitial(r.cfg);
        setPreset(r.preset);
        setFormKey((n) => n + 1);
      })
      .catch((e: unknown) => alive && setLoadError(backtestErrorText(t, errorMessage(e, "runNotFound"))));
    return () => {
      alive = false;
    };
    // `t` only words an error; a language switch must not reset the form.
  }, [presetId, fromRun, typeParam]);

  const switchKind = async (k: StrategyKind) => {
    if (k === kind) return;
    try {
      const cfg = await strategyDefault(k, "binance", form?.cfg.symbol || DEFAULT_SYMBOL);
      setKind(k);
      setPreset(null);
      setInitial({ ...cfg, market: form?.cfg.market ?? cfg.market, name: `${k === "dca" ? "DCA" : "Grid"} ${cfg.symbol}` });
      setForm(null);
      setFormKey((n) => n + 1);
    } catch (e) {
      setLoadError(strategyErrorText(t, errorMessage(e, "botUnknown")));
    }
  };

  const loadRuns = useCallback(() => {
    backtestList()
      .then((r) => {
        setRuns(r);
        setListError(null);
      })
      .catch((e: unknown) => setListError(backtestErrorText(t, errorMessage(e, "storeReadFailed"))));
  }, [t]);
  // Reloaded when a run ends, even one started before this page remounted.
  useEffect(loadRuns, [loadRuns, finished]);

  // Window in UTC days; the end is clipped to now by Rust as well.
  const now = Date.now();
  let startMs: number | null;
  let endMs: number | null;
  if (windowId === "custom") {
    const s = parseDay(customStart);
    const e = parseDay(customEnd);
    startMs = s;
    endMs = e !== null ? Math.min(now, e + DAY) : null;
  } else {
    const days = WINDOWS.find((w) => w.id === windowId)?.days ?? 90;
    endMs = now;
    startMs = now - days * DAY;
  }
  const windowOk = startMs !== null && endMs !== null && endMs > startMs;

  const canRun = signedIn && !running && !!form?.ok && windowOk;
  const [riskHeld, setRiskHeld] = useState(false);
  const run = async (confirmed = false) => {
    if (!canRun || !form || startMs === null || endMs === null) return;
    if (!confirmed && needsLiquidationConfirm(form.cfg, summary?.preview ?? null)) {
      setRiskHeld(true);
      return;
    }
    // Claimed synchronously: a double click or a second page cannot start
    // a second run while this one is pending.
    const token = newBacktestRunToken();
    if (!runStore.begin(token)) return;
    let unlisten: Unlisten = () => undefined;
    try {
      unlisten = await onBacktestProgress(token, (p) => runStore.progress(token, p));
      // A preset whose settings were changed is no longer that preset: the
      // run must not carry its id (the report would show the preset's name
      // over a 5x, no-stop variant of it).
      const config = preset && !matchesPreset(form.cfg, preset) ? { ...form.cfg, presetId: null } : form.cfg;
      const r = await backtestRun({
        config,
        symbol: form.cfg.symbol,
        market: form.cfg.market,
        interval,
        startMs,
        endMs,
        runToken: token,
      });
      runStore.end(token, null);
      // Only while the user is still on the run page: never pull them away
      // from another page when a run they left behind finishes.
      if (runStore.pageMounted()) navigate(`/backtest/${r.id}`);
    } catch (e) {
      runStore.end(token, errorMessage(e, "historyNetwork"));
    } finally {
      unlisten();
    }
  };

  const crumbs = [{ label: t("nav.groups.research") }];
  const runLabel = running
    ? progress && progress.total > 1
      ? t("backtest.runningPages", { done: progress.done, total: progress.total })
      : t("backtest.running")
    : t("backtest.run");
  const notAvailable = runError?.split("|")[0] === "historyNotAvailable";

  const runColumns: DataColumn<BacktestRunSummary>[] = [
    { id: "date", header: t("backtest.col.date"), cell: (r) => <span className="tabular">{formatTableTime(r.createdAt, locale)}</span> },
    {
      id: "strategy",
      header: t("backtest.col.strategy"),
      cell: (r) => (
        <>
          <Link to={`/backtest/${r.id}`} className="ae-link">
            {r.name}
          </Link>
          <span className="ae-bt__sub">{t(`botsList.type.${r.kind}`)}</span>
        </>
      ),
    },
    {
      id: "symbol",
      header: t("table.symbol"),
      priority: 2,
      cell: (r) => (
        <>
          {r.symbol}
          <span className="ae-bt__sub">
            {t(`filters.market.${r.market}`)} · {r.interval}
          </span>
        </>
      ),
    },
    { id: "window", header: t("backtest.window"), priority: 3, cell: (r) => <span className="tabular">{windowText(locale, r.startMs, r.endMs)}</span> },
    {
      id: "net",
      header: t("backtest.col.net"),
      numeric: true,
      cell: (r) => formatSignedPercent(r.totalPnlPct, locale),
      tone: (r) => pnlToneAttr(r.totalPnlPct),
    },
    {
      id: "maxDd",
      header: t("strategy.col.drawdownShort"),
      headerTip: t("backtest.col.maxDd"),
      detailLabel: t("backtest.col.maxDd"),
      numeric: true,
      priority: 2,
      cell: (r) => formatSignedPercent(r.maxDrawdownPct, locale),
    },
    {
      id: "cycles",
      header: t("backtest.col.cycles"),
      numeric: true,
      priority: 2,
      cell: (r) => (r.openAtEnd ? `${r.closedCycles} +1` : r.closedCycles),
    },
    {
      id: "coverage",
      header: t("backtest.col.coverage"),
      numeric: true,
      priority: 3,
      cell: (r) => (r.coveragePct !== null ? formatPercent(r.coveragePct, locale, 1) : NO_VALUE),
    },
    {
      id: "delete",
      header: "",
      keep: true,
      cell: (r) => (
        <Button
          size="xs"
          variant="ghost"
          aria-label={t("backtest.deleteRun", { name: r.name })}
          onClick={(e) => {
            e.stopPropagation();
            setDeleting({ id: r.id, name: r.name });
          }}
        >
          {t("backtest.delete")}
        </Button>
      ),
    },
  ];

  return (
    <PageShell
      title={t("nav.backtest")}
      crumbs={crumbs}
      wideLeft
      primary={
        <Button
          size="sm"
          disabled={!canRun}
          disabledReason={!signedIn ? t("backtest.signInRequired") : undefined}
          tooltipPlacement="bottom"
          tooltipAlign="end"
          aria-busy={running || undefined}
          onClick={() => void run()}
        >
          {runLabel}
        </Button>
      }
      left={
        <>
          <Panel title={t("backtest.settings")}>
            <div className="ae-sform__grid">
              <Segmented
                label={t("table.type")}
                value={kind}
                disabled={running || !!fromRun}
                options={[
                  { value: "dca", label: t("botsList.type.dca") },
                  { value: "grid", label: t("botsList.type.grid") },
                ]}
                onChange={(k) => void switchKind(k)}
              />
              <Segmented
                label={t("backtest.interval")}
                value={interval}
                disabled={running}
                options={BACKTEST_INTERVALS.map((v) => ({ value: v, label: v }))}
                onChange={setBarInterval}
                hint={t("backtest.hint.interval")}
              />
              {/* Six short windows: the same segmented control as Interval, wrapped into 3 + 3 when the column is narrow. */}
              <Segmented
                label={t("backtest.window")}
                value={windowId}
                disabled={running}
                options={WINDOWS.map((w) => ({ value: w.id, label: t(`backtest.windows.${w.id}`) }))}
                onChange={setWindowId}
                hint={windowOk && startMs !== null && endMs !== null ? <span className="tabular">{windowText(locale, startMs, endMs)}</span> : undefined}
              />
              {windowId === "custom" ? (
                <div className="ae-sform__pair">
                  <label className="ae-sfield">
                    <span className="ae-field__label">{t("backtest.from")}</span>
                    <input type="date" className="ae-field__input mono" value={customStart} max={customEnd} onChange={(e) => setCustomStart(e.target.value)} />
                  </label>
                  <label className="ae-sfield">
                    <span className="ae-field__label">{t("backtest.to")}</span>
                    <input type="date" className="ae-field__input mono" value={customEnd} min={customStart} max={utcDay(now)} onChange={(e) => setCustomEnd(e.target.value)} />
                  </label>
                </div>
              ) : null}
            </div>
            {!windowOk ? <p className="ae-sfield__error">{t("backtest.errors.windowInvalid")}</p> : null}
            <p className="ae-field__hint">{t("backtest.sourceHint")}</p>
          </Panel>
          {loadError ? (
            <p className="ae-banner" data-tone="danger" role="alert">
              {loadError}
            </p>
          ) : initial ? (
            <StrategyForm
              key={formKey}
              kind={kind}
              initial={initial}
              preset={preset}
              mode="backtest"
              onSubmit={async () => null}
              onCancel={() => undefined}
              onPreview={setSummary}
              onState={setForm}
            />
          ) : (
            <p className="ae-subtle">{t("workspace.loading")}</p>
          )}
        </>
      }
      right={
        initial ? (
          <>
            <StrategyPreviewPanel preview={summary?.preview ?? null} error={summary?.error ?? null} cfg={summary?.cfg ?? initial} available={null} backtest />
            {form ? (
              <LeverageRiskDialog
                open={riskHeld}
                cfg={form.cfg}
                preview={summary?.preview ?? null}
                onConfirm={() => {
                  setRiskHeld(false);
                  void run(true);
                }}
                onCancel={() => setRiskHeld(false)}
              />
            ) : null}
          </>
        ) : null
      }
    >
      <MetaLine
        items={[
          { text: t("backtest.label.historical") },
          { text: t("backtest.label.feesIncluded") },
          ...((form?.cfg.market ?? initial?.market) === "futures" ? [{ text: t("backtest.label.noFunding"), warn: true }] : []),
        ]}
      />
      {!signedIn ? (
        <EmptyState tone="pending" title={t("backtest.signInRequired")} detail={t("backtest.signInDetail")} />
      ) : null}
      {runError ? (
        notAvailable ? (
          <EmptyState tone="pending" title={t("backtest.errors.historyNotAvailable")} detail={t("backtest.notAvailableDetail")} />
        ) : (
          <p className="ae-banner" data-tone="danger" role="alert">
            {backtestErrorText(t, runError)}
          </p>
        )
      ) : null}

      <Panel
        title={t("backtest.runs")}
        aside={
          runs ? (
            <span className="ae-bt__runsaside">
              <span className="ae-muted">{t("backtest.runsKept", { n: runs.length, max: 50 })}</span>
              {runs.length > 0 ? (
                <Button size="xs" variant="secondary" onClick={() => setDeleting("all")}>
                  {t("backtest.deleteAll")}
                </Button>
              ) : null}
            </span>
          ) : null
        }
      >
        {listError ? (
          <p className="ae-banner" data-tone="danger">
            {listError}
          </p>
        ) : runs === null ? (
          <p className="ae-subtle">{t("workspace.loading")}</p>
        ) : runs.length === 0 ? (
          <EmptyState title={t("backtest.noRuns")} detail={t("backtest.noRunsDetail")} />
        ) : (
          <DataTable
            label={t("backtest.runs")}
            columns={runColumns}
            rows={runs}
            rowKey={(r) => r.id}
            onRowActivate={(r) => navigate(`/backtest/${r.id}`)}
            bare
          />
        )}
        {deleteError ? <p className="ae-error" role="alert">{deleteError}</p> : null}
      </Panel>
      <ConfirmDialog
        open={deleting !== null}
        title={deleting === "all" ? t("backtest.deleteAllTitle", { n: runs?.length ?? 0 }) : t("backtest.deleteTitle")}
        body={deleting && deleting !== "all" ? <p>{deleting.name}</p> : <p>{t("backtest.deleteAllBody")}</p>}
        confirmLabel={deleting === "all" ? t("backtest.deleteAll") : t("backtest.delete")}
        danger
        onCancel={() => setDeleting(null)}
        onConfirm={() => {
          const target = deleting;
          setDeleting(null);
          setDeleteError(null);
          const job = target === "all" ? backtestDeleteAll() : target ? backtestDelete(target.id) : Promise.resolve();
          void Promise.resolve(job)
            .then(loadRuns)
            .catch((e: unknown) => setDeleteError(backtestErrorText(t, errorMessage(e, "storeReadFailed"))));
        }}
      />
    </PageShell>
  );
}

function durationText(t: TFunction, ms: number): string {
  if (ms <= 0) return t("backtest.duration.none");
  const days = Math.floor(ms / DAY);
  const hours = Math.round((ms % DAY) / 3_600_000);
  return days > 0 ? t("backtest.duration.dh", { d: days, h: hours }) : t("backtest.duration.h", { h: Math.max(1, hours) });
}

/** #/backtest/:runId — one stored run: summary, equity, cycles. */
export function BacktestReportPage({ runId }: { runId: string }) {
  const { t } = useTranslation();
  const locale = useLocale();
  const [run, setRun] = useState<BacktestRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const crumbs = [{ label: t("nav.groups.research") }, { label: t("nav.backtest"), to: "/backtest" }];

  useEffect(() => {
    let alive = true;
    setRun(null);
    setError(null);
    backtestGet(runId)
      .then((r) => alive && setRun(r))
      .catch((e: unknown) => alive && setError(errorMessage(e, "runNotFound")));
    return () => {
      alive = false;
    };
  }, [runId]);

  if (error || !run) {
    return (
      <PageShell title={t("page.backtestReport")} crumbs={crumbs} single>
        {error ? (
          <EmptyState
            tone={error.startsWith("runNotFound") ? "default" : "error"}
            title={backtestErrorText(t, error)}
            detail={runId}
            actions={
              <Button variant="secondary" size="sm" onClick={() => navigate("/backtest")}>
                {t("nav.backtest")}
              </Button>
            }
          />
        ) : (
          <p className="ae-subtle">{t("workspace.loading")}</p>
        )}
      </PageShell>
    );
  }

  const r = run.result;
  const d = run.data;
  const cfg = run.config;
  const dca = cfg.params.kind === "dca";
  const usdt = (v: number, signed = false) => (signed ? formatSignedUsdt(v, locale) : formatUsdt(v, locale));
  const when = (ms: number) => new Date(ms).toLocaleString(locale, { timeZone: "UTC" });
  const pct = (v: number | null, digits = 1) => (v !== null ? formatPercent(v, locale, digits) : NO_VALUE);
  const tone = (v: number) => pnlToneAttr(v);

  const dataFacts: Fact[] = [
    { label: t("backtest.data.source"), value: d.source || "—" },
    { label: t("table.symbol"), value: `${run.symbol} · ${t(`filters.market.${run.market}`)}` },
    { label: t("backtest.interval"), value: run.interval },
    { label: t("backtest.window"), value: windowText(locale, run.startMs, run.endMs) },
    { label: t("backtest.data.candles"), value: formatNumber(d.candleCount, locale) },
    { label: t("backtest.data.expected"), value: d.expectedCandles !== null ? formatNumber(d.expectedCandles, locale) : "—" },
    { label: t("backtest.col.coverage"), value: pct(d.coveragePct), tone: d.coveragePct !== null && d.coveragePct < 95 ? "warn" : undefined },
    { label: t("backtest.data.missing"), value: formatNumber(d.missingBars, locale), tone: d.missingBars > 0 ? "warn" : undefined },
    { label: t("backtest.data.dropped"), value: formatNumber(d.droppedCandles, locale), tone: d.droppedCandles > 0 ? "warn" : undefined },
    { label: t("backtest.data.requests"), value: d.requests },
    { label: t("backtest.data.firstBar"), value: when(r.firstBarMs) },
    { label: t("backtest.data.lastBar"), value: when(r.lastBarMs) },
    { label: t("backtest.data.created"), value: new Date(run.createdAt).toLocaleString(locale) },
  ];
  const configFacts: Fact[] = [
    { label: t("table.name"), value: cfg.name },
    { label: t("table.type"), value: t(`botsList.type.${cfg.params.kind}`) },
    { label: t("table.side"), value: sideText(t, cfg.side) },
    { label: t("table.leverage"), value: `${cfg.leverage}x` },
    { label: t("strategy.field.budget"), value: usdt(cfg.budget) },
    ...(cfg.presetId ? [{ label: t("strategy.detail.preset"), value: cfg.presetId }] : []),
  ];

  type Cycle = (typeof r.cycles)[number];
  const cycleColumns: DataColumn<Cycle>[] = [
    { id: "seq", header: "#", numeric: true, width: "1%", cell: (c) => c.seq },
    { id: "opened", header: t("strategy.detail.opened"), priority: 2, cell: (c) => <span className="tabular">{when(c.openedAt)}</span> },
    { id: "closed", header: t("table.closedAt"), cell: (c) => <span className="tabular">{when(c.closedAt)}</span> },
    {
      id: "exit",
      header: t("table.exitReason"),
      cell: (c) => (c.openAtEnd ? <Chip tone="warn">{t("backtest.openAtEndChip")}</Chip> : t(`strategy.exit.${c.exit}`, { defaultValue: c.exit })),
    },
    { id: "anchor", header: t("strategy.detail.anchor"), numeric: true, priority: 3, cell: (c) => formatPrice(c.anchorPrice, locale) },
    {
      id: "avgEntry",
      header: t("strategy.preview.avgEntry"),
      numeric: true,
      priority: 2,
      cell: (c) => (c.avgEntry !== null ? formatPrice(c.avgEntry, locale) : NO_VALUE),
    },
    {
      id: "fills",
      header: t(dca ? "strategy.detail.soFilled" : "strategy.detail.closingFills"),
      numeric: true,
      priority: 3,
      cell: (c) => (dca ? c.soFilled : c.gridClosingFills) ?? NO_VALUE,
    },
    {
      id: "fees",
      header: t("strategy.detail.fees"),
      numeric: true,
      priority: 3,
      cell: (c) => formatNumber(c.feesQuote, locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }),
    },
    { id: "adverse", header: t("strategy.detail.maxAdverse"), numeric: true, priority: 3, cell: (c) => formatSignedPercent(c.maxAdversePct, locale) },
    {
      id: "pnlPct",
      header: t("table.pnlPct"),
      numeric: true,
      cell: (c) => formatSignedPercent(c.pnlPctBudget, locale, 3),
      tone: (c) => pnlToneAttr(c.pnlPctBudget, 3),
    },
    {
      id: "pnlUsdt",
      header: t("table.pnlUsdt"),
      numeric: true,
      cell: (c) => formatPnl(c.pnlQuote, locale, { unit: "none" }).text,
      tone: (c) => pnlToneAttr(c.pnlQuote),
    },
  ];

  return (
    <PageShell
      title={cfg.name}
      crumbs={crumbs}
      secondary={
        <>
          <Button size="sm" variant="ghost" onClick={() => setConfirmDelete(true)}>
            {t("backtest.delete")}
          </Button>
          <Button size="sm" variant="secondary" onClick={() => navigate(`/backtest?run=${run.id}`)}>
            {t("backtest.runAgain")}
          </Button>
        </>
      }
      right={
        <>
          <Panel title={t("backtest.data.title")}>
            <FactList rows={dataFacts} />
          </Panel>
          <Panel title={t("strategy.preset.settings")}>
            <FactList rows={configFacts} />
          </Panel>
        </>
      }
    >
      <MetaLine
        items={[
          { text: t("backtest.label.historical") },
          { text: t("backtest.label.feesIncluded") },
          ...(run.market === "futures" && !d.fundingIncluded ? [{ text: t("backtest.label.noFunding"), warn: true }] : []),
          { text: t("backtest.label.noGates") },
          // Equity is read at candle closes; paper bots run on 1m candles.
          { text: t("backtest.label.barCloses", { interval: run.interval }), warn: run.interval === "4h" || run.interval === "1d" },
          { text: t("backtest.label.coverage", { value: pct(d.coveragePct) }), warn: d.coveragePct !== null && d.coveragePct < 95 },
        ]}
      />

      <KpiGrid>
        <KpiTile label={t("backtest.kpi.net")} value={formatSignedPercent(r.totalPnlPct, locale)} tone={tone(r.totalPnlPct)} note={usdt(r.totalPnlQuote, true)} />
        <KpiTile
          label={t("backtest.kpi.closed")}
          value={r.closedCycles}
          note={t("backtest.kpi.closedNote", { wins: r.wins, losses: r.losses, pnl: usdt(r.closedPnlQuote, true) })}
        />
        <KpiTile
          label={t("backtest.kpi.openAtEnd")}
          value={r.openAtEnd ? usdt(r.openAtEndPnlQuote, true) : t("backtest.kpi.none")}
          tone={r.openAtEnd ? tone(r.openAtEndPnlQuote) : "muted"}
          note={r.openAtEnd ? t("backtest.kpi.openAtEndNote") : undefined}
        />
        <KpiTile label={t("backtest.col.maxDd")} value={formatSignedPercent(r.maxDrawdownPct, locale)} note={usdt(r.maxDrawdownQuote, true)} />
        <KpiTile label={t("backtest.kpi.underwater")} value={durationText(t, r.longestUnderwaterMs)} />
        <KpiTile label={t("strategy.detail.fees")} value={usdt(r.feesQuote)} />
        {dca ? (
          <KpiTile label={t("backtest.kpi.deepestSo")} value={r.deepestSo ?? 0} note={cfg.params.kind === "dca" ? t("backtest.kpi.ofMax", { max: cfg.params.maxSo }) : undefined} />
        ) : (
          <KpiTile label={t("strategy.detail.closingFills")} value={r.gridClosingFills ?? 0} note={t("backtest.kpi.fills", { n: r.totalFills })} />
        )}
        <KpiTile label={t("backtest.kpi.liquidations")} value={r.liquidations} tone={r.liquidations > 0 ? "down" : undefined} />
      </KpiGrid>

      {r.endState === "dead" || r.endState === "stopped" ? (
        <p className="ae-banner" data-tone="warn">
          {r.endState === "dead"
            ? t("backtest.endDead", { reason: r.deadReason ? t(`strategy.exit.${r.deadReason}`, { defaultValue: r.deadReason }) : "—" })
            : t("backtest.endStopped")}
        </p>
      ) : null}

      <Panel title={t("strategy.detail.equity")}>
        {r.equity.length < 2 ? (
          <p className="ae-subtle">{t("strategy.detail.noEquity")}</p>
        ) : (
          <>
            <EquityChart rows={r.equity} budget={r.budget} label={t("strategy.detail.equity")} />
            <MetaLine
              items={[
                { key: "range", text: `${when(r.equity[0].ts)} – ${when(r.equity[r.equity.length - 1].ts)} UTC` },
                { key: "base", text: t("strategy.detail.equityBase", { value: usdt(r.budget) }) },
              ]}
            />
          </>
        )}
      </Panel>

      {Object.keys(r.exits).length > 0 || Object.keys(r.notes).length > 0 ? (
        <Panel title={t("backtest.events")}>
          <FactList
            rows={[
              ...Object.entries(r.exits).map(([k, n]) => ({ label: t("strategy.stats.exit", { reason: t(`strategy.exit.${k}`, { defaultValue: k }) }), value: n })),
              ...Object.entries(r.notes).map(([k, n]) => ({ label: strategyNoteText(t, { key: k, detail: null }), value: n, tone: "muted" as const })),
            ]}
          />
        </Panel>
      ) : null}

      <Panel
        title={t("backtest.cycles")}
        aside={r.cyclesTruncated ? <span className="ae-muted">{t("backtest.cyclesTruncated", { n: r.cycles.length, total: r.closedCycles + (r.openAtEnd ? 1 : 0) })}</span> : null}
      >
        {r.cycles.length === 0 ? (
          <EmptyState title={t("strategy.detail.noCycles")} />
        ) : (
          <DataTable
            label={t("backtest.cycles")}
            columns={cycleColumns}
            rows={[...r.cycles].reverse()}
            rowKey={(c) => String(c.seq)}
            bare
            compact
          />
        )}
      </Panel>

      <ConfirmDialog
        open={confirmDelete}
        title={t("backtest.deleteTitle")}
        confirmLabel={t("backtest.delete")}
        danger
        busy={deleting}
        onConfirm={() => {
          setDeleting(true);
          backtestDelete(run.id)
            .then(() => navigate("/backtest"))
            .catch((e: unknown) => {
              setDeleting(false);
              setConfirmDelete(false);
              setError(errorMessage(e, "runNotFound"));
            });
        }}
        onCancel={() => setConfirmDelete(false)}
      />
    </PageShell>
  );
}
