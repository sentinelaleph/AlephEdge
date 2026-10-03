import { useTranslation } from "react-i18next";
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
  modeKey: string;
  to: string;
}

const CARDS: TypeCard[] = [
  { id: "signal", markets: ["spot", "futures"], suitsKey: "botCreate.suits.signal", modeKey: "botCreate.mode.signal", to: "/bots/signal" },
  { id: "dca", markets: ["spot", "futures"], suitsKey: "botCreate.suits.trend", modeKey: "botCreate.mode.paperOnly", to: "/bots/dca/new" },
  { id: "grid", markets: ["spot", "futures"], suitsKey: "botCreate.suits.range", modeKey: "botCreate.mode.paperOnly", to: "/bots/grid/new" },
];

/**
 * #/bots/new: the bot type catalog. Presets exist for DCA and Grid only
 * (strategy_presets); the signal bot has none, so it gets no presets link.
 * Three equal choices: each Create is secondary (no single primary here).
 */
export function BotNewPage() {
  const { t } = useTranslation();
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
                { label: t("table.mode"), value: t(c.modeKey) },
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
