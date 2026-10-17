import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { KEYS_PATH } from "@/app/router/routes";
import { Link, navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip, LiveChip, PaperChip } from "@/components/ui/Chip/Chip";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel, type Fact } from "@/components/ui/Panel/Panel";
import { InfoTip } from "@/components/ui/InfoTip/InfoTip";
import { localeForLanguage } from "@/i18n";
import { formatSignedPercent, formatUsdt, NO_VALUE } from "@/lib/format";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { isActive, type Preset, type StrategyKind } from "@/lib/ipc/strategy/strategy";
import { presetName } from "@/lib/strategyText";
import { presetFormPath, provenPreset, testSplit, usePresets } from "@/pages/Presets/presetUi";
import { freeStrategyBudget, templateCounts } from "./botNewModel";
import { signalBotRows } from "./botRows";
import { useStrategyLive } from "./strategy/useStrategyLive";
import "./bots.css";

interface TypeCard {
  id: "signal" | StrategyKind;
  markets: ("spot" | "futures")[];
  suitsKey: string;
}

const CARDS: TypeCard[] = [
  { id: "signal", markets: ["spot", "futures"], suitsKey: "botCreate.suits.signal" },
  { id: "dca", markets: ["spot", "futures"], suitsKey: "botCreate.suits.trend" },
  { id: "grid", markets: ["spot", "futures"], suitsKey: "botCreate.suits.range" },
];

const GUIDE_REAL_MONEY = "/guide?s=real-money";
const PREFLIGHT_PATH = "/risk?s=preflight";
/** Exchanges with a real-money order path in the code; real money stays off on each until its test-network run passes. */
const PENDING_VENUES = ["bybit", "okx"];

/**
 * The mode line of a bot type. Real money is offered only by a build that
 * can place real orders (`--features live`); the public build is paper only
 * and says so, without advertising an opt-in it does not have.
 */
export function modeKey(liveTradingEnabled: boolean): string {
  return liveTradingEnabled ? "botCreate.mode.liveBuildLine" : "botCreate.mode.paperOnly";
}

/** A card's tested result: one sentence, too long for a label / value row. */
function Evidence({ fact }: { fact: Fact }) {
  return (
    <div className="ae-newbot__evidence" data-tone={fact.tone}>
      <span className="ae-newbot__evidencehead">
        {fact.label}
        {fact.info ? <InfoTip label={fact.label} content={fact.info} size="sm" placement="top" align="center" /> : null}
      </span>
      <span className="tabular">{fact.value}</span>
    </div>
  );
}

/** The card's first button: the proven template's form, when the type has one. */
function ProvenStart({ p }: { p: Preset | null }) {
  const { t } = useTranslation();
  if (!p) return null;
  return (
    <Button size="sm" onClick={() => navigate(presetFormPath(p))}>
      {t("botCreate.startProven", { name: presetName(t, p) })}
    </Button>
  );
}

/**
 * #/bots/new: the bot type catalog. Each card states what the type does,
 * what its templates proved in the historical test, and whether it can trade
 * real money in this build; the mode panel says how paper and real money
 * work; the right panel lists what a new bot starts from (risk level, free
 * budget, keys, real-money venues).
 */
export function BotNewPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk, strategy, keyedExchanges, catalog } = useDeskContext();
  const { presets } = usePresets();
  const { liveIds } = useStrategyLive();
  const liveBuild = desk.loaded && desk.status.liveTradingEnabled;
  const venues = desk.loaded ? (desk.status.liveVenues ?? []) : [];
  const venueNames = venues.map((v) => exchangeName(v, catalog.exchanges)).join(", ");
  // Venues with an order path whose test-network run has not passed yet.
  const pendingVenues = PENDING_VENUES.filter((v) => !venues.includes(v)).map((v) => exchangeName(v, catalog.exchanges)).join(", ");
  const signalRows = desk.loaded ? signalBotRows(desk.status, risk.state?.maxCapitalQuote ?? null).filter((r) => r.config) : null;
  const pct = (v: number, digits = 2) => formatSignedPercent(v, locale, digits);

  /** "2 · running 1 · 1 LIVE", "None yet", or null while the desk loads. */
  const yours = (id: TypeCard["id"]): string | null => {
    const mine =
      id === "signal"
        ? signalRows && {
            total: signalRows.length,
            running: signalRows.filter((r) => r.running).length,
            live: signalRows.filter((r) => r.live).length,
          }
        : strategy.bots && {
            total: strategy.bots.filter((b) => b.kind === id).length,
            running: strategy.bots.filter((b) => b.kind === id && isActive(b)).length,
            live: strategy.bots.filter((b) => b.kind === id && liveIds.has(b.id)).length,
          };
    if (!mine) return null;
    if (mine.total === 0) return t("botCreate.yoursNone");
    const base = t("botCreate.yoursValue", mine);
    return mine.live > 0 ? `${base} · ${t("botCreate.yoursLive", { n: mine.live })}` : base;
  };

  const templatesFact = (kind: StrategyKind): Fact => {
    if (!presets) return { label: t("botCreate.templates"), value: NO_VALUE };
    const c = templateCounts(presets, kind);
    if (c.total === 0) return { label: t("botCreate.templates"), value: t("botCreate.templatesNone") };
    const parts = [
      c.proven ? t("botCreate.templatesProven", { n: c.proven }) : null,
      c.experimental ? t("botCreate.templatesExperimental", { n: c.experimental }) : null,
    ].filter(Boolean);
    return { label: t("botCreate.templates"), value: parts.join(" · "), tone: c.proven === 0 ? "warn" : undefined };
  };

  const evidenceFact = (id: TypeCard["id"]): Fact => {
    const label = t("botCreate.evidence");
    if (id === "signal") return { label, value: t("botCreate.evidenceSignal"), tone: "muted" };
    if (!presets) return { label, value: NO_VALUE };
    const best = provenPreset(presets, id);
    const s = best ? testSplit(best) : null;
    if (!best || !s) return { label, value: t("botCreate.evidenceNone"), tone: "warn" };
    return {
      label,
      value: t("botCreate.evidenceProven", {
        name: presetName(t, best),
        mean: pct(s.meanPerBotMonthPct),
        lo: pct(s.ci95LowPct),
        hi: pct(s.ci95HighPct),
        dd: pct(best.history.testWorstBotDrawdownPct, 1),
      }),
      info: t("botCreate.evidenceInfo"),
    };
  };

  /** The proven template of a strategy type (DCA only today), the card's first button. */
  const proven = (id: TypeCard["id"]) => (id !== "signal" && presets ? provenPreset(presets, id) : null);

  const realFact = (): Fact => ({
    label: t("botCreate.realMoney"),
    value: !desk.loaded ? NO_VALUE : liveBuild && venues.length ? t("botCreate.realFutures", { venues: venueNames }) : t("botCreate.realNotInBuild"),
    tone: liveBuild && venues.length ? undefined : "muted",
  });

  const free = freeStrategyBudget(strategy.risk);
  const level = risk.state?.limits.level;
  const keyed = keyedExchanges?.map((e) => e.name).join(", ");
  const liveSignal = signalRows?.filter((r) => r.live).length ?? 0;
  const liveTotal = liveSignal + liveIds.size;

  const right = (
    <>
      <Panel title={t("botCreate.ready.title")}>
        <FactList
          rows={[
            {
              label: t("botCreate.ready.risk"),
              value: risk.state
                ? t("botCreate.ready.riskValue", {
                    level: t(`risk.levels.${level}`),
                    lev: risk.state.limits.maxLeverage,
                    pos: risk.state.limits.maxConcurrentPositions,
                  })
                : NO_VALUE,
            },
            {
              label: t("botCreate.ready.budget"),
              value: free !== null && strategy.risk ? t("botCreate.ready.budgetValue", { free: formatUsdt(free, locale), cap: strategy.risk.budgetCapPct }) : NO_VALUE,
              tone: free !== null && free < 5 ? "warn" : undefined,
            },
            {
              label: t("botCreate.ready.keys"),
              value: keyedExchanges === null ? NO_VALUE : keyed ? keyed : t("botCreate.ready.keysNone"),
              tone: keyed ? undefined : "muted",
            },
            {
              label: t("botCreate.realMoney"),
              value: !desk.loaded ? NO_VALUE : liveBuild ? (venues.length ? t("botCreate.realFutures", { venues: venueNames }) : t("botCreate.ready.noVenue")) : t("botCreate.realNotInBuild"),
              tone: liveBuild ? undefined : "muted",
            },
            ...(liveBuild ? [{ label: t("botCreate.ready.liveBots"), value: String(liveTotal), tone: liveTotal > 0 ? ("danger" as const) : undefined }] : []),
          ]}
        />
        <p className="ae-newbot__links">
          <Link to="/risk" className="ae-link">
            {t("botCreate.ready.toRisk")}
          </Link>
          <Link to={KEYS_PATH} className="ae-link">
            {t("botCreate.ready.toKeys")}
          </Link>
        </p>
      </Panel>
    </>
  );

  return (
    <PageShell title={t("page.botNew")} crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]} right={right}>
      <Panel title={t("botCreate.mode.title")} aside={liveBuild ? <LiveChip /> : <PaperChip />} className="ae-newbot__mode">
        {liveBuild ? (
          <div className="ae-newbot__modes">
            <div>
              <p className="ae-newbot__modehead">
                <PaperChip /> {t("botCreate.mode.paperHead")}
              </p>
              <p className="ae-typegrid__how">{t("botCreate.mode.paperBody")}</p>
            </div>
            <div>
              <p className="ae-newbot__modehead">
                <LiveChip /> {t("botCreate.mode.realHead")}
              </p>
              <p className="ae-typegrid__how">{t("botCreate.mode.realBody", { venues: venueNames || NO_VALUE })}</p>
              <ol className="ae-newbot__steps">
                <li>
                  {t("botCreate.mode.step1")} <Link to={KEYS_PATH} className="ae-link">{t("botCreate.ready.toKeys")}</Link>
                </li>
                <li>
                  {t("botCreate.mode.step2")} <Link to={PREFLIGHT_PATH} className="ae-link">{t("botCreate.mode.preflight")}</Link>
                </li>
                <li>{t("botCreate.mode.step3")}</li>
                <li>{t("botCreate.mode.step4")}</li>
              </ol>
              <p className="ae-muted ae-newbot__fine">
                {pendingVenues ? `${t("botCreate.mode.notYet", { venues: pendingVenues })} ` : null}
                <Link to={GUIDE_REAL_MONEY} className="ae-link">{t("botCreate.mode.guide")}</Link>
              </p>
            </div>
          </div>
        ) : (
          <p className="ae-typegrid__how">
            {t(modeKey(false))} <Link to={GUIDE_REAL_MONEY} className="ae-link">{t("botCreate.mode.guide")}</Link>
          </p>
        )}
      </Panel>

      <div className="ae-typegrid">
        {CARDS.map((c) => (
          <Panel
            key={c.id}
            title={t(`botCreate.type.${c.id}`)}
            aside={
              <span className="ae-typegrid__chips">
                {c.markets.map((m) => (
                  <Chip key={m}>{t(`filters.market.${m}`)}</Chip>
                ))}
              </span>
            }
          >
            <p className="ae-typegrid__how">{t(`botCreate.how.${c.id}`)}</p>
            <FactList
              rows={[
                { label: t("botCreate.suitsLabel"), value: t(c.suitsKey) },
                { label: t("botCreate.yours"), value: yours(c.id) ?? NO_VALUE },
                c.id === "signal" ? { label: t("botCreate.templates"), value: t("botCreate.templatesSignal"), tone: "muted" } : templatesFact(c.id),
                realFact(),
              ]}
            />
            <Evidence fact={evidenceFact(c.id)} />
            <div className="ae-typegrid__foot">
              {c.id === "signal" ? (
                <Button variant="secondary" size="sm" onClick={() => navigate("/bots/signal")}>
                  {t("botCreate.openSignal")}
                </Button>
              ) : (
                <>
                  <ProvenStart p={proven(c.id)} />
                  <Button variant="secondary" size="sm" onClick={() => navigate(`/presets?type=${c.id}`)}>
                    {t("botCreate.fromTemplate")}
                  </Button>
                  <Button variant="secondary" size="sm" onClick={() => navigate(`/bots/${c.id}/new`)}>
                    {t("botCreate.blank")}
                  </Button>
                </>
              )}
            </div>
          </Panel>
        ))}
      </div>
    </PageShell>
  );
}
