import { useTranslation } from "react-i18next";
import { useDeskContext } from "@/app/DeskProvider";
import { Link } from "@/app/router/router";
import { Button } from "@/components/ui/Button/Button";
import { LiveChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn } from "@/components/ui/DataTable/DataTable";
import { StatusChip } from "@/components/ui/StatusChip/StatusChip";
import { Tooltip } from "@/components/ui/Tooltip/Tooltip";
import { localeForLanguage } from "@/i18n";
import { exchangeName } from "@/lib/ipc/exchange/exchange";
import { formatUsdt, NO_VALUE } from "@/lib/format";
import { rowStatus, type BotRow } from "./botRows";
import "./bots.css";

export type SortKey = "openPositions" | "capital" | "maxPositions";
export interface SortState {
  key: SortKey;
  dir: "asc" | "desc";
}

interface BotsTableProps {
  rows: BotRow[];
  sort?: SortState | null;
  onSort?: (s: SortState | null) => void;
  selected?: Set<string>;
  onSelect?: (next: Set<string>) => void;
}

const SORT_KEYS: SortKey[] = ["openPositions", "capital", "maxPositions"];

/** The signal bot state chip; a locked or refused state explains itself on hover. */
export function BotStateChip({ row }: { row: BotRow }) {
  const { t } = useTranslation();
  const s = rowStatus(row);
  const chip = <StatusChip status={s.status} label={t(s.labelKey)} />;
  if (!s.reasonKey) return chip;
  return (
    <Tooltip content={t(s.reasonKey)} focusable>
      {chip}
    </Tooltip>
  );
}

/**
 * Start / Stop for one signal bot row (xs secondary). A refused Start keeps
 * its label and says why in a tooltip, never as a red line under the row.
 */
export function SignalRowAction({ row }: { row: BotRow }) {
  const { t } = useTranslation();
  const { desk } = useDeskContext();
  if (row.running) {
    return (
      <Button variant="secondary" size="xs" disabled={desk.busy} onClick={() => void desk.stop(row.kind)}>
        {t("bots.stop")}
      </Button>
    );
  }
  return (
    <Button
      variant="secondary"
      size="xs"
      disabled={desk.busy || row.startBlockKey !== null}
      disabledReason={row.startBlockKey ? t(row.startBlockKey) : undefined}
      tooltipAlign="end"
      // Same configure-then-start chain as the settings form, with the saved config.
      onClick={() => row.config && void desk.configureAndStart(row.config)}
    >
      {t("bots.start")}
    </Button>
  );
}

/** The signal bots table of #/bots. */
export function BotsTable({ rows, sort, onSort, selected, onSelect }: BotsTableProps) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const { catalog } = useDeskContext();

  const toggle = (id: string) => {
    if (!onSelect) return;
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onSelect(next);
  };

  const columns: DataColumn<BotRow>[] = [
    ...(onSelect
      ? [
          {
            id: "select",
            header: "",
            width: "1%",
            cell: (row: BotRow) => (
              <input
                type="checkbox"
                aria-label={t("botsList.selectRow", { name: t(`bots.kind.${row.kind}`) })}
                checked={selected?.has(row.id) ?? false}
                onChange={() => toggle(row.id)}
              />
            ),
          },
        ]
      : []),
    {
      id: "name",
      header: t("table.name"),
      keep: true,
      cell: (row) => (
        <span className="ae-namecell">
          <Link to={`/bots/${row.id}`} className="ae-link">
            {t(`bots.kind.${row.kind}`)}
          </Link>
          {row.live ? <LiveChip /> : null}
        </span>
      ),
    },
    { id: "type", header: t("table.type"), priority: 3, cell: (row) => t(`botsList.type.${row.type}`) },
    { id: "market", header: t("table.market"), priority: 2, cell: (row) => t(`filters.market.${row.market}`) },
    {
      id: "exchange",
      header: t("table.exchange"),
      priority: 3,
      cell: (row) => (row.exchangeId ? exchangeName(row.exchangeId, catalog.exchanges) : NO_VALUE),
    },
    {
      id: "pairs",
      header: t("table.pairs"),
      priority: 3,
      cell: (row) => (row.config ? (row.symbols.length === 0 ? t("botsList.allPairs") : row.symbols.join(", ")) : NO_VALUE),
    },
    { id: "state", header: t("table.state"), cell: (row) => <BotStateChip row={row} /> },
    {
      id: "openPositions",
      header: t("table.openPositions"),
      numeric: true,
      sortable: true,
      priority: 2,
      cell: (row) => row.openPositions,
    },
    {
      id: "capital",
      header: t("table.capital"),
      numeric: true,
      sortable: true,
      priority: 2,
      cell: (row) => (row.capital !== null ? formatUsdt(row.capital, locale) : NO_VALUE),
    },
    {
      id: "maxPositions",
      header: t("table.maxPositions"),
      numeric: true,
      sortable: true,
      priority: 3,
      cell: (row) => row.maxPositions ?? NO_VALUE,
    },
  ];

  const onSortColumn = onSort
    ? (id: string) => {
        if (!SORT_KEYS.includes(id as SortKey)) return;
        const key = id as SortKey;
        const on = sort?.key === key;
        onSort(on ? (sort?.dir === "desc" ? { key, dir: "asc" } : null) : { key, dir: "desc" });
      }
    : undefined;

  return (
    <DataTable
      label={t("nav.signalBots")}
      columns={columns}
      rows={rows}
      rowKey={(row) => row.id}
      isSelected={selected ? (row) => selected.has(row.id) : undefined}
      sort={sort ? { id: sort.key, dir: sort.dir } : null}
      onSort={onSortColumn}
      // The name links to the bot; the row keeps only its run action.
      actions={(row) => <SignalRowAction row={row} />}
    />
  );
}

export function sortRows(rows: BotRow[], sort: SortState | null): BotRow[] {
  if (!sort) return rows;
  const val = (r: BotRow) => (sort.key === "openPositions" ? r.openPositions : sort.key === "capital" ? r.capital ?? -1 : r.maxPositions ?? -1);
  return [...rows].sort((a, b) => (sort.dir === "desc" ? val(b) - val(a) : val(a) - val(b)));
}
