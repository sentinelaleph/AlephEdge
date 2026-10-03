import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { KillSwitchCard, RegimeCard } from "@/components/Desk/Readouts";
import { LINK_STATUS } from "@/components/Link/linkStatus";
import { RiskSelector } from "@/components/RiskSelector/RiskSelector";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { PageShell } from "@/components/ui/PageShell/PageShell";
import { FactList, Panel } from "@/components/ui/Panel/Panel";
import { Section } from "@/components/ui/Section/Section";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { formatNumber } from "@/lib/format";
import type { BotKind } from "@/lib/ipc/bot/bot";
import { linkState, type LinkStateView } from "@/lib/ipc/link/link";
import { RISK_LEVELS, RISK_LIMITS_TABLE, type RiskLevel } from "@/lib/ipc/risk/risk";
import { StrategyEmergency } from "@/pages/Bots/strategy/StrategyEmergency";
import { StrategyRiskPanel } from "@/pages/Bots/strategy/StrategyRiskPanel";
import "./RiskPage.css";

const KINDS: BotKind[] = ["futures", "spot", "pump"];
const SECTIONS = ["level", "table", "killSwitch", "regime", "live", "emergency"] as const;
/** Levels whose Pump bot is allowed (the pump gate in the Rust risk rules). */
const PUMP_LEVELS: RiskLevel[] = ["ambitious", "greedy"];

/**
 * #/risk: every limit and safety control in one place. Changes apply at once
 * (the risk setters), turning LIVE OFF is always allowed here, and turning it
 * ON stays on the Futures card behind the typed LIVE word.
 */
export function RiskPage() {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { risk, desk, vault, endpoints } = useDeskContext();
  const s = desk.status;
  const [liveError, setLiveError] = useState<string | null>(null);
  const level = risk.state?.limits.level;
  const binanceKey = vault.credentials.some((c) => c.exchangeId === "binance");
  const running = KINDS.filter((k) => (k === "futures" ? s.futuresRunning : k === "spot" ? s.spotRunning : s.pumpRunning));
  const capital = s.openPositions.reduce((n, p) => n + p.capital, 0);
  const balance = risk.state?.balance ?? null;

  const limitColumns: DataColumn<RiskLevel>[] = [
    {
      id: "level",
      header: t("safety.level.title"),
      cell: (l) => (
        <span className="ae-risklevel" data-level={l}>
          {t(`risk.levels.${l}`)}
        </span>
      ),
    },
    { id: "leverage", header: t("safety.limits.leverage"), numeric: true, cell: (l) => `${RISK_LIMITS_TABLE[l].maxLeverage}x` },
    { id: "positions", header: t("safety.limits.positions"), numeric: true, priority: 2, cell: (l) => RISK_LIMITS_TABLE[l].maxConcurrentPositions },
    { id: "capital", header: t("safety.limits.capital"), numeric: true, priority: 2, cell: (l) => `${RISK_LIMITS_TABLE[l].maxCapitalPct}%` },
    { id: "dailyLoss", header: t("safety.limits.dailyLoss"), numeric: true, cell: (l) => `-${RISK_LIMITS_TABLE[l].dailyLossLimitPct}%` },
    { id: "fr", header: t("safety.limits.fr"), numeric: true, priority: 3, cell: (l) => `${RISK_LIMITS_TABLE[l].frThresholdPct}%` },
    { id: "depth", header: t("safety.limits.depth"), numeric: true, priority: 3, cell: (l) => `${RISK_LIMITS_TABLE[l].maxDepthSharePct}%` },
    {
      id: "pump",
      header: t("bots.kind.pump"),
      priority: 2,
      cell: (l) =>
        PUMP_LEVELS.includes(l) ? (
          <StatusChip status="ok" label={t("safety.available")} />
        ) : (
          <StatusChip status="idle" label={t("status.locked")} />
        ),
    },
  ];

  const turnOff = async (kind: BotKind) => {
    setLiveError(await desk.setLive(kind, false, ""));
  };

  const left = (
    <Panel title={t("safety.sections")}>
      <ul className="ae-list">
        {SECTIONS.map((id) => (
          <li key={id}>
            <a
              href={`#/risk`}
              className="ae-link"
              onClick={(e) => {
                e.preventDefault();
                document.getElementById(`risk-${id}`)?.scrollIntoView({ block: "start" });
              }}
            >
              {t(`safety.section.${id}`)}
            </a>
          </li>
        ))}
      </ul>
    </Panel>
  );

  const right = (
    <>
      <RemoteControlCard />
      <Panel title={t("safety.exposure")}>
        <FactList
          rows={[
            { label: t("table.openPositions"), value: s.openPositions.length },
            { label: t("positions.capitalInUse"), value: `${formatNumber(capital, locale, { maximumFractionDigits: 2 })} USDT` },
            {
              label: t("positions.exposure"),
              value: balance ? `${formatNumber((capital / balance) * 100, locale, { maximumFractionDigits: 1 })}%` : "—",
            },
          ]}
        />
      </Panel>
    </>
  );

  return (
    <PageShell title={t("nav.risk")} crumbs={[{ label: t("nav.groups.control") }]} left={left} right={right}>
      <section id="risk-level" className="ae-risk-section">
        <RiskSelector risk={risk} />
      </section>

      <section id="risk-table" className="ae-risk-section">
        <Section title={t("safety.section.table")}>
          <DataTable
            label={t("safety.section.table")}
            columns={limitColumns}
            rows={[...RISK_LEVELS]}
            rowKey={(l) => l}
            isSelected={(l) => l === level}
            compact
          />
        </Section>
      </section>

      <section id="risk-killSwitch" className="ae-risk-section">
        <KillSwitchCard />
      </section>

      <section id="risk-regime" className="ae-risk-section">
        <RegimeCard />
      </section>

      <section id="risk-live" className="ae-risk-section">
        <Panel title={t("safety.section.live")} tone={KINDS.some((k) => s[k]?.live) ? "live" : "default"}>
          {!desk.loaded ? (
            <p className="ae-subtle">{t("workspace.loading")}</p>
          ) : !s.liveTradingEnabled ? (
            <p className="ae-muted">{t("safety.live.notThisBuild")}</p>
          ) : (
            <>
              <FactList
                rows={[
                  { label: t("safety.live.build"), value: t("safety.yes") },
                  { label: t("safety.live.binanceKey"), value: binanceKey ? t("safety.yes") : t("safety.no"), tone: binanceKey ? undefined : "warn" },
                  {
                    label: t("safety.live.endpoint"),
                    value: endpoints ? (endpoints.binanceIsProduction ? t("accounts.production") : t("app.testnetBadge")) : "—",
                  },
                ]}
              />
              <ul className="ae-list">
                {KINDS.map((k) => (
                  <li key={k}>
                    <span>{t(`bots.kind.${k}`)}</span>
                    {s[k]?.live ? (
                      <span className="ae-toolbar">
                        <LiveChip />
                        <Button variant="secondary" size="xs" disabled={desk.busy} onClick={() => void turnOff(k)}>
                          {t("safety.live.turnOff")}
                        </Button>
                      </span>
                    ) : null}
                  </li>
                ))}
              </ul>
              <p className="ae-subtle">
                {t("safety.live.onWhere")}{" "}
                <Link to="/bots/signal" className="ae-link">
                  {t("nav.signalBots")}
                </Link>
              </p>
              {liveError ? <p className="ae-error">{liveError}</p> : null}
            </>
          )}
        </Panel>
      </section>

      <section id="risk-emergency" className="ae-risk-section">
        <Panel title={t("safety.section.emergency")} tone="danger">
          <div className="ae-toolbar">
            <Button
              variant="secondary"
              size="sm"
              disabled={desk.busy || running.length === 0}
              onClick={() => {
                for (const k of running) void desk.stop(k);
              }}
            >
              {t("safety.emergency.stopAll", { count: running.length })}
            </Button>
            <Link to="/positions?tab=exchange" className="ae-link">
              {t("safety.emergency.realCloses")}
            </Link>
          </div>
          <p className="ae-subtle">{t("safety.emergency.stopNote")}</p>
          {desk.error ? <p className="ae-error">{desk.error}</p> : null}
          <h3 className="ae-section-title">{t("strategy.emergency.title")}</h3>
          <StrategyEmergency />
        </Panel>
        <StrategyRiskPanel />
      </section>
    </PageShell>
  );
}

/** Phone remote control: read-only link state (pairing lives in Settings > Devices). */
function RemoteControlCard() {
  const { t } = useTranslation();
  const [state, setState] = useState<LinkStateView | null>(null);

  useEffect(() => {
    let alive = true;
    let timer: number | undefined;
    const tick = async () => {
      try {
        const s = await linkState();
        if (alive) setState(s);
      } catch {
        /* keep last */
      }
      if (alive) timer = window.setTimeout(tick, 10_000);
    };
    void tick();
    return () => {
      alive = false;
      if (timer) window.clearTimeout(timer);
    };
  }, []);

  return (
    <Panel
      title={t("safety.remote")}
      aside={
        state ? <StatusChip status={LINK_STATUS[state.state]} label={t(`link.state.${state.state}`)} /> : null
      }
    >
      {state?.state === "stopped" ? <p className="ae-error">{state.message}</p> : null}
      <Link to="/settings?tab=devices" className="ae-link ae-muted">
        {t("settings.tabs.devices")}
      </Link>
    </Panel>
  );
}
