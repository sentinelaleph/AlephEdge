import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate, useRouter } from "@/app/router/router";
import { withQuery } from "@/app/router/routes";
import { Button } from "@/components/ui/Button/Button";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FilterGroup, listParam, SearchField } from "@/components/ui/FilterPanel/FilterPanel";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { localeForLanguage } from "@/i18n";
import { formatUsdt, NO_VALUE } from "@/lib/format";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { runStateText } from "@/lib/strategyText";
import { signalBotRows, type BotRow, type BotType } from "./botRows";
import { BotsTable, sortRows, type SortState } from "./BotsTable";
import { SkeletonRows } from "./SkeletonRows";
import { isActive, type StrategyBotView } from "@/lib/ipc/strategy/strategy";
import { StrategyBotsTable } from "./strategy/StrategyBotsTable";
import { useStrategyActions } from "./strategy/useStrategyActions";
import { useStrategyLive } from "@/pages/Bots/strategy/useStrategyLive";
import { MarketTrendPanel } from "@/components/Desk/MarketTrend";

const SORT_KEY = "aleph-edge-bots-sort";

function readSort(): SortState | null {
  try {
    const raw = localStorage.getItem(SORT_KEY);
    return raw ? (JSON.parse(raw) as SortState) : null;
  } catch {
    return null;
  }
}

const TYPES: BotType[] = ["signal", "dca", "grid"];
const STATES = ["running", "stopped", "stoppedByKillSwitch", "notConfigured"] as const;

/** #/bots: every bot of every type in one table, filters in the URL. */
export function BotsPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk, catalog, strategy } = useDeskContext();
  const strategyActions = useStrategyActions();
  const { route } = useRouter();
  const q = route.query;
  const [sort, setSortState] = useState<SortState | null>(readSort);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const setSort = (s: SortState | null) => {
    setSortState(s);
    try {
      if (s) localStorage.setItem(SORT_KEY, JSON.stringify(s));
      else localStorage.removeItem(SORT_KEY);
    } catch {
      /* sort not remembered */
    }
  };

  const filters = {
    q: q.get("q") ?? "",
    type: listParam(q.get("type")),
    state: listParam(q.get("state")),
    market: listParam(q.get("market")),
    mode: listParam(q.get("mode")),
    exchange: listParam(q.get("exchange")),
  };
  const setFilter = (name: keyof typeof filters, value: string | string[]) => {
    const next: Record<string, string> = {};
    for (const [k, v] of Object.entries({ ...filters, [name]: value })) {
      next[k] = Array.isArray(v) ? v.join(",") : v;
    }
    navigate(withQuery("/bots", next), { replace: true });
  };
  const anyFilter = Object.values(filters).some((v) => (Array.isArray(v) ? v.length > 0 : v !== ""));

  const all: BotRow[] = useMemo(
    () => (desk.loaded ? signalBotRows(desk.status, risk.state?.maxCapitalQuote ?? null) : []),
    [desk.loaded, desk.status, risk.state?.maxCapitalQuote],
  );
  const rows = sortRows(
    all.filter((r) => {
      const name = t(`bots.kind.${r.kind}`).toLowerCase();
      const needle = filters.q.trim().toLowerCase();
      if (needle && !name.includes(needle) && !r.symbols.some((s) => s.toLowerCase().includes(needle))) return false;
      if (filters.type.length && !filters.type.includes(r.type)) return false;
      if (filters.state.length && !filters.state.includes(r.state)) return false;
      if (filters.market.length && !filters.market.includes(r.market)) return false;
      if (filters.mode.length && !filters.mode.includes(r.live ? "live" : "paper")) return false;
      if (filters.exchange.length && !filters.exchange.includes(r.exchangeId ?? "")) return false;
      return true;
    }),
    sort,
  );
  // DCA / Grid instances under the same filters; a bot on real money is "live".
  const { liveIds: strategyLiveIds } = useStrategyLive();
  const strategyAll: StrategyBotView[] = strategy.bots ?? [];
  const strategyState = (b: StrategyBotView) => (isActive(b) || b.openCycle ? "running" : "stopped");
  const strategyRows = strategyAll.filter((b) => {
    const needle = filters.q.trim().toLowerCase();
    if (needle && !b.name.toLowerCase().includes(needle) && !b.symbol.toLowerCase().includes(needle)) return false;
    if (filters.type.length && !filters.type.includes(b.kind)) return false;
    if (filters.state.length && !filters.state.includes(strategyState(b))) return false;
    if (filters.market.length && !filters.market.includes(b.market)) return false;
    if (filters.mode.length && !filters.mode.includes(strategyLiveIds.has(b.id) ? "live" : "paper")) return false;
    if (filters.exchange.length && !filters.exchange.includes(b.exchangeId)) return false;
    return true;
  });
  const exchanges = [
    ...new Set([...all.map((r) => r.exchangeId), ...strategyAll.map((b) => b.exchangeId)].filter((x): x is string => !!x)),
  ];
  const runningSelected = rows.filter((r) => selected.has(r.id) && r.running);
  const allSelected = rows.length > 0 && rows.every((r) => selected.has(r.id));

  const left = (
    <>
      <SearchField label={t("filters.search")} value={filters.q} onChange={(v) => setFilter("q", v)} />
      <FilterGroup
        label={t("filters.type")}
        options={TYPES.map((v) => ({
          value: v,
          label: t(`botsList.type.${v}`),
          count: v === "signal" ? all.length : strategyAll.filter((b) => b.kind === v).length,
        }))}
        selected={filters.type}
        onChange={(v) => setFilter("type", v)}
      />
      <FilterGroup
        label={t("filters.state")}
        options={STATES.map((v) => ({
          value: v,
          label: t(`botState.${v}`),
          count: all.filter((r) => r.state === v).length + strategyAll.filter((b) => strategyState(b) === v).length,
        }))}
        selected={filters.state}
        onChange={(v) => setFilter("state", v)}
      />
      <FilterGroup
        label={t("filters.marketLabel")}
        options={(["spot", "futures"] as const).map((v) => ({ value: v, label: t(`filters.market.${v}`) }))}
        selected={filters.market}
        onChange={(v) => setFilter("market", v)}
      />
      <FilterGroup
        label={t("filters.mode")}
        options={[
          { value: "paper", label: t("states.paper") },
          { value: "live", label: "LIVE" },
        ]}
        selected={filters.mode}
        onChange={(v) => setFilter("mode", v)}
      />
      {exchanges.length > 0 ? (
        <FilterGroup
          label={t("filters.exchange")}
          options={exchanges.map((v) => ({ value: v, label: exchangeName(v, catalog.exchanges) }))}
          selected={filters.exchange}
          onChange={(v) => setFilter("exchange", v)}
        />
      ) : null}
      {anyFilter ? (
        <Button variant="ghost" size="sm" onClick={() => navigate("/bots", { replace: true })}>
          {t("states.clearFilters")}
        </Button>
      ) : null}
    </>
  );

  const configured = all.filter((r) => r.config);
  const attention = all.filter((r) => r.state === "stoppedByKillSwitch");
  const strategyAttention = strategyAll.filter((b) => b.runState === "dead" || b.runState === "paused");
  const right = (
    <>
      <MarketTrendPanel />
      {TYPES.map((type) => {
        if (type !== "signal") {
          const of = strategyAll.filter((b) => b.kind === type);
          return (
            <Panel key={type} title={t(`botsList.type.${type}`)}>
              <FactList
                rows={[
                  { label: t("botsList.totals.bots"), value: strategy.bots ? of.length : NO_VALUE },
                  { label: t("botsList.totals.running"), value: strategy.bots ? of.filter(isActive).length : NO_VALUE },
                  { label: t("strategy.list.openCycles"), value: strategy.bots ? of.filter((b) => b.openCycle).length : NO_VALUE },
                  { label: t("strategy.list.budgetTotal"), value: formatUsdt(of.reduce((sum, b) => sum + b.budget, 0), locale) },
                ]}
              />
            </Panel>
          );
        }
        const of = all.filter((r) => r.type === type);
        const cap = of.reduce((sum, r) => sum + (r.config ? r.capital ?? 0 : 0), 0);
        return (
          <Panel key={type} title={t(`botsList.type.${type}`)}>
            <FactList
              rows={[
                { label: t("botsList.totals.bots"), value: type === "signal" ? configured.length : of.length },
                { label: t("botsList.totals.running"), value: of.filter((r) => r.running).length },
                { label: t("table.openPositions"), value: of.reduce((n, r) => n + r.openPositions, 0) },
                ...(type === "signal"
                  ? [{ label: t("botsList.totals.capitalAllocated"), value: formatUsdt(cap, locale) }]
                  : []),
              ]}
            />
          </Panel>
        );
      })}
      <Panel title={t("botsList.attention")}>
        {attention.length === 0 && strategyAttention.length === 0 && !desk.error ? (
          <p className="ae-subtle">{t("botsList.attentionNone")}</p>
        ) : (
          <ul className="ae-list">
            {attention.map((r) => (
              <li key={r.id}>
                <Link to={`/bots/${r.id}`} className="ae-link">
                  {t(`bots.kind.${r.kind}`)}
                </Link>
                <span className="ae-error">{t(`botState.${r.state}`)}</span>
              </li>
            ))}
            {strategyAttention.map((b) => (
              <li key={b.id}>
                <Link to={`/bots/${b.id}`} className="ae-link">
                  {b.name}
                </Link>
                <span className="ae-error">{runStateText(t, b)}</span>
              </li>
            ))}
            {desk.error ? <li className="ae-error">{desk.error}</li> : null}
          </ul>
        )}
      </Panel>
    </>
  );

  return (
    <PageShell
      title={t("nav.allBots")}
      crumbs={[{ label: t("nav.groups.bots") }]}
      primary={
        <Button size="sm" onClick={() => navigate("/bots/new")}>
          {t("nav.newBot")}
        </Button>
      }
      left={left}
      right={right}
    >
      {desk.error ? (
        <p className="ae-banner" data-tone="danger" role="alert">
          {desk.error}
        </p>
      ) : null}
      {!desk.loaded ? (
        <SkeletonRows rows={6} />
      ) : rows.length === 0 && strategyRows.length === 0 ? (
        anyFilter ? (
          <EmptyState
            title={t("botsList.noMatch")}
            actions={
              <Button variant="secondary" size="sm" onClick={() => navigate("/bots", { replace: true })}>
                {t("states.clearFilters")}
              </Button>
            }
          />
        ) : (
          <EmptyState
            title={t("botsList.noBots")}
            actions={
              <>
                <Button size="sm" onClick={() => navigate("/bots/new")}>
                  {t("nav.newBot")}
                </Button>
                <Button variant="secondary" size="sm" onClick={() => navigate("/presets")}>
                  {t("botsList.browsePresets")}
                </Button>
              </>
            }
          />
        )
      ) : (
        <>
          {selected.size > 0 ? (
            <div className="ae-toolbar">
              <span className="ae-muted">{t("botsList.selectedCount", { count: selected.size })}</span>
              <Button
                variant="secondary"
                size="sm"
                disabled={desk.busy || runningSelected.length === 0}
                onClick={() => {
                  for (const r of runningSelected) void desk.stop(r.kind);
                  setSelected(new Set());
                }}
              >
                {t("botsList.bulkStop", { count: runningSelected.length })}
              </Button>
            </div>
          ) : null}
          {rows.length > 0 ? (
            <Section
              title={t("nav.signalBots")}
              count={rows.length}
              aside={
                <label className="ae-checkline">
                  <input
                    type="checkbox"
                    checked={allSelected}
                    onChange={() => setSelected(allSelected ? new Set() : new Set(rows.map((r) => r.id)))}
                  />
                  {t("botsList.selectAll")}
                </label>
              }
            >
              <BotsTable rows={rows} sort={sort} onSort={setSort} selected={selected} onSelect={setSelected} />
            </Section>
          ) : null}
          {strategyRows.length > 0 ? (
            <Section title={t("strategy.list.section")} count={strategyRows.length}>
              <StrategyBotsTable rows={strategyRows} actions={strategyActions} showType />
            </Section>
          ) : null}
        </>
      )}
      {strategyActions.dialog}
    </PageShell>
  );
}
