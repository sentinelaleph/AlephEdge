import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Icon } from "@/app/AppShell/icons";
import { Link } from "@/app/router/router";
import { Button, IconButton } from "@/components/ui/Button/Button";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { StatusChip, type StatusKind } from "@/components/ui/StatusChip/StatusChip";
import { localeForLanguage } from "@/i18n";
import { formatNumber, formatPnl, formatSignedPercent, pnlToneAttr } from "@/lib/format";
import { isActive, type StrategyBotView } from "@/lib/ipc/strategy/strategy";
import { RUN_TONE, runActionKey, runStateText, sideText } from "@/lib/strategyText";
import { LiveChip } from "@/components/ui/Chip/Chip";
import type { StrategyActions } from "./useStrategyActions";
import { useStrategyLive } from "./useStrategyLive";

interface Props {
  rows: StrategyBotView[];
  actions: StrategyActions;
  /** Show the Type column (All bots mixes DCA and Grid). */
  showType?: boolean;
}

/** Total P&L (realized + floating, after fees), % of budget. */
export function totalPnlPct(v: StrategyBotView): number {
  return ((v.equity - v.budget) / v.budget) * 100;
}

export function runTone(v: StrategyBotView) {
  if (v.openCycle && !v.acceptingNewCycles && v.runState !== "dead" && v.runState !== "paused") return "paused" as const;
  return RUN_TONE[v.runState];
}

/** The StatusChip state of a DCA / Grid bot (tone map: StatusChip/statusTone.ts). */
export function runStatusKind(v: StrategyBotView): StatusKind {
  if (v.runState === "dead") return "dead";
  if (v.openCycle && !v.acceptingNewCycles && v.runState !== "paused") return "closingOnly";
  switch (v.runState) {
    case "inCycle":
      return "running";
    case "armed":
      return "armed";
    case "paused":
      return "paused";
    default:
      return "stopped";
  }
}

/**
 * DCA / Grid bot rows (paper only; the header's mode chip says so). Every
 * figure is served by Rust. Reference usage of DataTable + StatusChip +
 * formatPnl: low-priority columns fold into the row detail on narrow
 * windows, the row actions stay pinned right.
 */
export function StrategyBotsTable({ rows, actions, showType }: Props) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { strategy } = useDeskContext();
  const { liveIds } = useStrategyLive();

  const columns: DataColumn<StrategyBotView>[] = [
    {
      id: "name",
      header: t("table.name"),
      cell: (v) => (
        <>
          <Link to={`/bots/${v.id}`} className="ae-link">
            {v.name}
          </Link>
          {liveIds.has(v.id) ? <> <LiveChip /></> : null}
        </>
      ),
    },
    ...(showType
      ? [{ id: "type", header: t("table.type"), priority: 2 as const, cell: (v: StrategyBotView) => t(`botsList.type.${v.kind}`) }]
      : []),
    { id: "symbol", header: t("table.symbol"), priority: 2, cell: (v) => v.symbol },
    {
      id: "market",
      header: t("table.market"),
      priority: 2,
      cell: (v) =>
        `${t(`filters.market.${v.market}`)} · ${sideText(t, v.side)}${v.leverage > 1 ? ` · ${v.leverage}x` : ""}`,
    },
    {
      id: "state",
      header: t("table.state"),
      cell: (v) => <StatusChip status={runStatusKind(v)} label={runStateText(t, v)} />,
    },
    {
      id: "budget",
      header: t("strategy.field.budget"),
      numeric: true,
      priority: 3,
      cell: (v) => formatNumber(v.budget, locale, { maximumFractionDigits: 2 }),
    },
    {
      id: "total",
      header: t("strategy.col.totalPnl"),
      numeric: true,
      cell: (v) => formatSignedPercent(totalPnlPct(v), locale),
      tone: (v) => pnlToneAttr(totalPnlPct(v)),
    },
    {
      id: "realized",
      header: t("strategy.col.realized"),
      numeric: true,
      priority: 2,
      cell: (v) => formatPnl(v.realizedQuote, locale, { unit: "none" }).text,
      tone: (v) => pnlToneAttr(v.realizedQuote),
    },
    {
      id: "drawdown",
      header: t("strategy.col.drawdownShort"),
      headerTip: t("strategy.col.drawdown"),
      detailLabel: t("strategy.col.drawdown"),
      numeric: true,
      priority: 3,
      cell: (v) => `${formatSignedPercent(v.drawdownPct, locale)} / ${formatSignedPercent(v.maxDrawdownPct, locale)}`,
    },
    { id: "cycles", header: t("strategy.col.cycles"), numeric: true, priority: 3, cell: (v) => v.cyclesDone },
  ];

  return (
    <DataTable
      label={t("nav.groups.bots")}
      columns={columns}
      rows={rows}
      rowKey={(v) => v.id}
      rowNote={(v) => (actions.error?.botId === v.id ? actions.error.text : null)}
      rowTone={(v) => (actions.error?.botId === v.id ? "danger" : undefined)}
      // The name links to the bot; the row keeps its run action and Delete
      // (a paper bot only: one on real money goes back to paper first).
      actions={(v) => {
        const busy = strategy.busyId === v.id;
        const run =
          v.runState === "dead" ? null : isActive(v) ? (
            <Button variant="secondary" size="xs" disabled={busy} onClick={() => actions.request(v, "stop")}>
              {t("strategy.actions.stop")}
            </Button>
          ) : (
            <Button variant="secondary" size="xs" disabled={busy} onClick={() => actions.request(v, "start")}>
              {t(runActionKey(v))}
            </Button>
          );
        return (
          <>
            {run}
            {liveIds.has(v.id) ? null : (
              <IconButton
                size="xs"
                data-tone="danger"
                label={t("strategy.actions.archive")}
                icon={<Icon name="trash" size={14} />}
                tooltipPlacement="top"
                tooltipAlign="end"
                disabled={busy}
                onClick={() => actions.request(v, "archive")}
              />
            )}
          </>
        );
      }}
    />
  );
}
