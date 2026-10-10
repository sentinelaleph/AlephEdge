import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { Chip } from "@/components/ui/Chip/Chip";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
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
  const { desk } = useDeskContext();
  const mode = modeKey(desk.loaded && desk.status.liveTradingEnabled);
  return (
    <PageShell title={t("page.botNew")} crumbs={[{ label: t("nav.groups.bots"), to: "/bots" }]} single>
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
            <FactList
              rows={[
                { label: t("botCreate.suitsLabel"), value: t(c.suitsKey) },
                { label: t("table.mode"), value: t(mode) },
              ]}
            />
            <div className="ae-typegrid__foot">
              {c.id !== "signal" ? (
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
