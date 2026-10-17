/**
 * Pure rules of the "Before you trade" page (unit-tested): which label a
 * figure wears, whether a figure may be shown at all, and how a measured
 * news impact is said. The page never computes a figure of its own.
 */

import type {
  ChartRead,
  FarmRanking,
  ImpactHorizon,
  LabTemplate,
  LeafStatus,
  ListingRow,
  Presets,
} from "@/lib/ipc/beforeYouTrade/beforeYouTrade";

/** i18n key of the BTC share figure: what the number actually is. */
export function dominanceLabelKey(kind: string): string {
  if (kind === "spot_volume_share") return "byt.market.btcSpotShare";
  if (kind === "market_cap") return "byt.market.btcMarketCapDominance";
  return "byt.market.btcDominanceUnknown";
}

/** Chip tone per leaf status. */
export function statusTone(s: LeafStatus | string): "success" | "waiting" | "neutral" | "warn" {
  if (s === "ok") return "success";
  if (s === "measuring") return "waiting";
  if (s === "not_started") return "neutral";
  return "warn";
}

/**
 * The chart read is shown only with its 24h hit record and its bar: a call
 * without the record would look like advice. Null = show it; otherwise the
 * i18n key of why not.
 */
export function chartReadHiddenReason(r: ChartRead): string | null {
  if (r.status !== "ok") return "byt.chart.notOk";
  if (!r.hit_record_24h || !r.bar) return "byt.chart.noRecord";
  if (!r.call) return "byt.chart.noCall";
  return null;
}

/** The farm's ranking may be shown only when the payload itself allows it. */
export function farmRankingShown(r: FarmRanking | undefined): boolean {
  return !!r && r.status === "allowed" && r.reasons.length === 0 && r.ranked.length > 0;
}

/** T01..T12 as the payload gives them; never re-sorted by a result. */
export function templatesInOrder(p: Presets): LabTemplate[] {
  return p.lab.templates;
}

/** The test split of a template, if the payload has one. */
export function testSplit(t: LabTemplate) {
  return t.splits.find((s) => s.split === "test") ?? null;
}

/** "Proven" only for the template the payload marks proven. */
export function isProven(t: LabTemplate): boolean {
  return t.claim === "proven";
}

/** New bot form of a template (the Presets page's path). */
export function templateFormPath(t: Pick<LabTemplate, "kind" | "preset_id">): string {
  return `/bots/${t.kind}/new?preset=${encodeURIComponent(t.preset_id)}`;
}

export type ImpactLine =
  | { kind: "measured"; n: number; median: number; shareUp: number; baseShareUp: number; differs: boolean; ci: [number, number] | null }
  | { kind: "measuring"; n: number; needed: number | null }
  | { kind: "none" };

/** One horizon of a news type's measured impact, as the page says it. */
export function impactLine(h: ImpactHorizon | undefined): ImpactLine {
  if (!h) return { kind: "none" };
  if (h.status === "measuring") return { kind: "measuring", n: h.n, needed: h.needed ?? null };
  if (h.median === undefined || h.share_up === undefined || h.base_share_up === undefined) return { kind: "none" };
  return {
    kind: "measured",
    n: h.n,
    median: h.median,
    shareUp: h.share_up,
    baseShareUp: h.base_share_up,
    differs: h.differs === true,
    ci: h.diff_ci ?? null,
  };
}

export interface ReactionCell {
  /** "measured" | "pending" | "noData" */
  kind: "measured" | "pending" | "noData";
  values: { horizon: "5m" | "1h" | "24h"; pct: number | null }[];
  reason: string | null;
}

/** A listing's measured reaction, or "No data" with the payload's reason. */
export function reactionCell(row: ListingRow): ReactionCell {
  const r = row.reaction;
  if (!r || r.status === "no_data") return { kind: "noData", values: [], reason: r?.reason ?? null };
  if (r.status === "pending") return { kind: "pending", values: [], reason: r.reason ?? null };
  const values = [
    { horizon: "5m" as const, pct: r.ret_5m_pct },
    { horizon: "1h" as const, pct: r.ret_1h_pct },
    { horizon: "24h" as const, pct: r.ret_24h_pct },
  ];
  if (values.every((v) => v.pct === null)) return { kind: "noData", values: [], reason: r.reason ?? null };
  return { kind: "measured", values, reason: r.reason ?? null };
}

/** Seconds since an RFC 3339 time (null when absent or unreadable). */
export function ageSecs(iso: string | null | undefined, now: number): number | null {
  if (!iso) return null;
  const t = Date.parse(iso);
  return Number.isFinite(t) ? Math.max(0, (now - t) / 1000) : null;
}
