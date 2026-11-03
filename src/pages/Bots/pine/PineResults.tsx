import { useTranslation } from "react-i18next";
import { LiveChip, PaperChip } from "@/components/ui/Chip/Chip";
import { DataTable, type DataColumn, type PriorityBreakpoints } from "@/components/ui/DataTable/DataTable";
import { EmptyState } from "@/components/ui/EmptyState/EmptyState";
import { Panel } from "@/components/ui/Panel/Panel";
import { localeForLanguage } from "@/i18n";
import { formatPrice, formatSignedPercent, formatTableTime, NO_VALUE } from "@/lib/format";
import type { PineResult } from "@/lib/ipc/pine/pine";
import { pineCodeText, triggerLabel } from "./pineText";

const BREAKPOINTS: PriorityBreakpoints = { p2: 560, p3: 720 };
const DONE = new Set(["opened", "closed", "flipped"]);

/**
 * What each bar's mapped triggers did, newest first (`pine_results`): the
 * trigger, the bar it fired on and its close, the fill and its slippage
 * against that close, and the outcome code in words.
 */
export function PineResults({ rows, error }: { rows: PineResult[] | null; error: string | null }) {
  const { t, i18n } = useTranslation();
  const locale = localeForLanguage(i18n.resolvedLanguage ?? "en");
  const exists = (k: string) => i18n.exists(k);
  const col = (x: string) => `botsPine.col.${x}`;

  const columns: DataColumn<PineResult>[] = [
    { id: "at", header: t(col("time")), keep: true, cell: (r) => formatTableTime(r.handledAt, locale) },
    { id: "symbol", header: t(col("symbol")), keep: true, cell: (r) => r.symbol },
    {
      id: "trigger",
      header: t(col("trigger")),
      wrap: true,
      cell: (r) => (
        <span className="ae-namecell">
          {r.triggers.map((f) => `${triggerLabel(t, f)} → ${t(`botsPine.action.${f.action}`)}`).join(" · ") || NO_VALUE}
          {r.mode === "live" ? <LiveChip /> : <PaperChip />}
        </span>
      ),
    },
    { id: "bar", header: t(col("bar")), priority: 2, cell: (r) => formatTableTime(r.barTime, locale) },
    { id: "close", header: t(col("close")), numeric: true, cell: (r) => formatPrice(r.barClose, locale) },
    { id: "fill", header: t(col("fill")), numeric: true, cell: (r) => (r.fillPrice == null ? NO_VALUE : formatPrice(r.fillPrice, locale)) },
    {
      id: "slip",
      header: t(col("slippage")),
      headerTip: t(col("slippageTip")),
      numeric: true,
      cell: (r) => (r.slippagePct == null ? NO_VALUE : formatSignedPercent(r.slippagePct, locale, 2)),
      tone: (r) => (r.slippagePct != null && r.slippagePct > 0 ? "down" : undefined),
    },
    {
      id: "outcome",
      header: t(col("outcome")),
      keep: true,
      wrap: true,
      cell: (r) => (r.note ? `${pineCodeText(t, exists, r.outcome)} · ${pineCodeText(t, exists, r.note)}` : pineCodeText(t, exists, r.outcome)),
      tone: (r) => (DONE.has(r.outcome) ? undefined : "down"),
    },
  ];

  return (
    <Panel title={t("botsPine.results")}>
      {error ? (
        <p className="ae-error" role="alert">
          {error}
        </p>
      ) : null}
      <DataTable
        label={t("botsPine.results")}
        columns={columns}
        rows={rows ?? []}
        rowKey={(r) => r.key}
        bare
        compact
        breakpoints={BREAKPOINTS}
        empty={<EmptyState title={rows === null ? t("botsPine.resultsLoading") : t("botsPine.resultsEmpty")} />}
      />
    </Panel>
  );
}
