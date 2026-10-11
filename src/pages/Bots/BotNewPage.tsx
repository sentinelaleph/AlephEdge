import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { NO_VALUE } from "@/lib/format";
import { isActive } from "@/lib/ipc/strategy/strategy";
import { usePresets, verdictBucket } from "@/pages/Presets/presetUi";
import { signalBotRows } from "./botRows";
import "./bots.css";

interface TypeCard {
  id: "signal" | "dca" | "grid";
  markets: ("spot" | "futures")[];
  suitsKey: string;
  to: string;
}

const CARDS: TypeCard[] = [
  { id: "signal", markets: ["spot", "futures"], suitsKey: "botCreate.suits.signal", to: "/bots/signal" },
  { id: "dca", markets: ["spot", "futures"], suitsKey: "botCreate.suits.trend", to: "/bots/dca/new" },
  { id: "grid", markets: ["spot", "futures"], suitsKey: "botCreate.suits.range", to: "/bots/grid/new" },
];

/**
 * The mode line of a bot type. Real money is offered only by a build that
 * can place real orders (`--features live`); the public build is paper only
 * and says so, without advertising an opt-in it does not have.
 */
export function modeKey(liveTradingEnabled: boolean): string {
  return liveTradingEnabled ? "botCreate.mode.signal" : "botCreate.mode.paperOnly";
}

/**
 * #/bots/new: the bot type catalog. Presets exist for DCA and Grid only
 * (strategy_presets); the signal bot has none, so it gets no presets link.
 * Three equal choices: each Create is secondary (no single primary here).
 */
export function BotNewPage() {
  const { t } = useTranslation();
  const { desk, risk, strategy } = useDeskContext();
  const { presets } = usePresets();
  const mode = modeKey(desk.loaded && desk.status.liveTradingEnabled);
  const signalRows = desk.loaded ? signalBotRows(desk.status, risk.state?.maxCapitalQuote ?? null).filter((r) => r.config) : null;

  /** "2 · running 1", "None yet", or null while the desk loads. */
  const yours = (id: TypeCard["id"]): string | null => {
    const mine =
      id === "signal"
        ? signalRows && { total: signalRows.length, running: signalRows.filter((r) => r.running).length }
        : strategy.bots && {
            total: strategy.bots.filter((b) => b.kind === id).length,
            running: strategy.bots.filter((b) => b.kind === id && isActive(b)).length,
          };
    if (!mine) return null;
    return mine.total === 0 ? t("botCreate.yoursNone") : t("botCreate.yoursValue", mine);
  };
  const templates = (id: "dca" | "grid"): string | null => {
    if (!presets) return null;
    const of = presets.filter((p) => p.config.params.kind === id);
    return of.length === 0 ? t("botCreate.templatesNone") : t("botCreate.templatesValue", { passed: of.filter((p) => verdictBucket(p) === "passed").length, total: of.length });
  };

  return (
    <PageShell title={t("page.botNew")} crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]} single>
      <p className="ae-subtle">{t(mode)}</p>
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
                ...(c.id === "signal" ? [] : [{ label: t("botCreate.templates"), value: templates(c.id) ?? NO_VALUE }]),
              ]}
            />
            <div className="ae-typegrid__foot">
              {c.id !== "signal" && presets?.some((p) => p.config.params.kind === c.id) ? (
                <Link to={`/presets?type=${c.id}`} className="ae-link ae-muted">
                  {t("botCreate.presetsLink")}
                </Link>
              ) : (
                <span />
              )}
              <Button variant="secondary" size="sm" onClick={() => navigate(c.to)}>
                {t("botCreate.create")}
              </Button>
            </div>
          </Panel>
        ))}
      </div>
    </PageShell>
  );
}
