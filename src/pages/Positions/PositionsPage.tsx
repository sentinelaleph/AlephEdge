import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link, navigate, useQueryParam, useRouter } from "@/app/router/router";
import { KEYS_PATH, withQuery } from "@/app/router/routes";
import { AccountPanel } from "@/components/Account/AccountPanel/AccountPanel";
import { LiveChip } from "@/components/ui/Chip/Chip";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { FilterGroup, listParam, SearchField } from "@/components/ui/FilterPanel/FilterPanel";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { Tabs } from "@/components/ui/Tabs/Tabs";
import { localeForLanguage } from "@/i18n";
import { formatPercent, formatUsdt, NO_VALUE } from "@/lib/format";
import type { BotKind } from "@/lib/ipc/bot/bot";
import { SkeletonRows } from "@/pages/Bots/SkeletonRows";
import { PaperCloseAll } from "./PaperCloseAll";
import { PositionsTable } from "./PositionsTable";
import { StrategyCyclesTable } from "./StrategyCyclesTable";
import "./Positions.css";

const KINDS: BotKind[] = ["futures", "spot", "pump"];

/**
 * #/positions: everything open, paper and real strictly apart. The real
 * exchange account (and its real close orders) lives only on the Exchange
 * tab, inside a red zone; the Paper tab never shows a real-order control.
 */
export function PositionsPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { desk, risk, accountExchanges, strategy } = useDeskContext();
  const [tabParam, setTab] = useQueryParam("tab");
  const tab = tabParam === "exchange" ? "exchange" : "paper";
  const { route } = useRouter();
  const kind = listParam(route.query.get("kind"));
  const side = listParam(route.query.get("side"));
  const sym = route.query.get("q") ?? "";
  const set = (patch: Record<string, string>) =>
    navigate(
      withQuery("/positions", {
        tab: tab === "exchange" ? "exchange" : "",
        kind: kind.join(","),
        side: side.join(","),
        q: sym,
        ...patch,
      }),
      { replace: true },
    );

  // Paper only: a LIVE bot's positions are real and show on the Exchange tab.
  const all = desk.status.openPositions.filter((p) => !p.live);
  const rows = all.filter(
    (p) =>
      (!kind.length || kind.includes(p.botKind)) &&
      (!side.length || side.includes(p.direction)) &&
      (!sym || p.symbol.toLowerCase().includes(sym.trim().toLowerCase())),
  );
  // DCA / Grid bots holding a cycle (paper only). The bot filter names signal
  // bot kinds, so a set bot filter leaves these out.
  const needle = sym.trim().toLowerCase();
  const cycleBots = (strategy.bots ?? []).filter(
    (b) =>
      b.openCycle !== null &&
      !kind.length &&
      (!side.length || side.includes(b.side)) &&
      (!needle || b.symbol.toLowerCase().includes(needle) || b.name.toLowerCase().includes(needle)),
  );
  const notional = rows.reduce((n, p) => n + (p.notionalUsdt > 0 ? p.notionalUsdt : 0), 0);
  const capital = rows.reduce((n, p) => n + p.capital, 0);
  const balance = risk.state?.balance ?? null;

  const paperLeft = (
    <>
      <SearchField label={t("filters.symbol")} value={sym} onChange={(v) => set({ q: v })} />
      <FilterGroup
        label={t("filters.bot")}
        options={KINDS.map((k) => ({ value: k, label: t(`bots.kind.${k}`), count: all.filter((p) => p.botKind === k).length }))}
        selected={kind}
        onChange={(v) => set({ kind: v.join(",") })}
      />
      <FilterGroup
        label={t("filters.direction")}
        options={(["long", "short"] as const).map((d) => ({ value: d, label: t(`signalDesk.direction.${d}`) }))}
        selected={side}
        onChange={(v) => set({ side: v.join(",") })}
      />
    </>
  );

  const paperRight = (
    <Panel title={t("positions.totals")}>
      <FactList
        rows={[
          { label: t("table.openPositions"), value: rows.length },
          { label: t("positions.strategyCycles"), value: strategy.bots === null ? NO_VALUE : cycleBots.length },
          { label: t("positions.notional"), value: formatUsdt(notional, locale, 0) },
          { label: t("positions.capitalInUse"), value: formatUsdt(capital, locale) },
          {
            label: t("positions.exposure"),
            value: balance ? formatPercent((capital / balance) * 100, locale, 1) : NO_VALUE,
          },
        ]}
      />
      <p className="ae-subtle">{t("positions.floatingPending")}</p>
    </Panel>
  );

  return (
    <PageShell
      title={t("nav.positions")}
      crumbs={[{ label: t("nav.groups.portfolio") }]}
      left={tab === "paper" ? paperLeft : undefined}
      right={tab === "paper" ? paperRight : undefined}
    >
      <Tabs
        label={t("nav.positions")}
        value={tab}
        onChange={(v) => setTab(v === "paper" ? null : v)}
        tabs={[
          { id: "paper", label: t("positions.tabPaper") },
          {
            id: "exchange",
            label: (
              <span className="ae-postab">
                {t("table.exchange")}
                <LiveChip />
              </span>
            ),
          },
        ]}
      >
        {tab === "paper" ? (
          !desk.loaded ? (
            <SkeletonRows rows={4} />
          ) : (
            <>
              <div className="ae-toolbar ae-positions__emergency">
                <PaperCloseAll />
              </div>
              <Section title={t("table.openPositions")} count={rows.length}>
                <PositionsTable positions={rows} showBot onClose={desk.closePosition} />
              </Section>
              <Section title={t("positions.strategyCycles")} count={strategy.bots === null ? undefined : cycleBots.length}>
                {strategy.bots === null ? <SkeletonRows rows={2} /> : <StrategyCyclesTable bots={cycleBots} />}
              </Section>
            </>
          )
        ) : accountExchanges.length === 0 ? (
          <EmptyState
            title={t("positions.noKey")}
            actions={
              <Link to={KEYS_PATH} className="ae-link">
                {t("settings.tabs.keys")}
              </Link>
            }
          />
        ) : (
          <Panel tone="live" title={t("positions.realZone")}>
            {accountExchanges.map((id) => (
              <AccountPanel key={id} exchangeId={id} />
            ))}
          </Panel>
        )}
      </Tabs>
    </PageShell>
  );
}
