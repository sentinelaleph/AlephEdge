/**
 * "Before you trade" (mirrors src-tauri/src/app/before_you_trade.rs): one
 * read-only Sentinel payload, schema "before-you-trade/1", passed through
 * from Rust untouched. The contract below is the server's
 * (beforeyoutrade/NOTES.md). Every leaf carries `status` and, when it is not
 * "ok", a `reason`: the page shows that reason instead of a number.
 */

import { devMock, inTauri, invoke } from "../bridge";

export type LeafStatus = "ok" | "measuring" | "not_started" | "unavailable";

export interface Leaf {
  status: LeafStatus;
  as_of: string | null;
  source: string;
  reason?: string;
}

export interface ScannerGuard {
  trusted: boolean;
  new_alt_longs_blocked: boolean;
  blocked_by_current_read: boolean;
  sticky_until: string | null;
  rule: string;
}

export interface BtcRegime extends Leaf {
  price: number | null;
  change_24h_pct: number | null;
  change_1h_pct: number | null;
  trend_state: "up" | "down" | "flat" | "";
  trend_rule: string;
  trend_strength: number | null;
  volatility_regime: "high" | "low" | "";
  volatility_rule: string;
  btc_dominance_pct: number | null;
  dominance_kind: "market_cap" | "spot_volume_share" | "";
  dominance_source: string;
  alt_long_risk: number | null;
  down_day_streak: number | null;
  down_day_halt_active: boolean | null;
  scanner_guard: ScannerGuard | null;
}

export type ReadCall = "higher" | "lower" | "range" | "";

export interface HitRecord {
  prompt_version: string;
  n: number;
  days: number;
  hits: number;
  hit_pct: number | null;
  always_range_pct: number | null;
  persistence_pct: number | null;
  rule_pct: number | null;
  directional_n: number;
  directional_pct: number | null;
}

export interface ChartRead extends Leaf {
  latest_attempt_at: string | null;
  latest_attempt_status: string;
  prompt_version: string;
  model: string;
  price_at_read: number | null;
  call: ReadCall;
  confidence: string;
  horizon_view: null | { h4: ReadCall; h12: ReadCall; h24: ReadCall };
  support: number[];
  resistance: number[];
  invalidation_level: number | null;
  hit_record_24h: HitRecord | null;
  bar: null | { status: "open" | "met" | "not_met" | "unavailable"; verdict: string; min_n: number };
  use: string;
}

export interface FundingRow {
  symbol: string;
  rate_pct: number;
  interval_hours: number;
  annualized_pct: number;
  next_funding: string;
  mark_price: number | null;
}

export interface Market {
  generated_at: string;
  btc_regime: BtcRegime;
  chart_read: ChartRead;
  volatility: {
    range_24h: Leaf & { high: number | null; low: number | null; range_pct: number | null };
    atr14_1d: Leaf & {
      atr_usd: number | null;
      atr_pct: number | null;
      last_close: number | null;
      bars_used: number;
      last_bar_end: string | null;
      method: string;
    };
  };
  funding: Leaf & { datahub_stale: boolean; rows: FundingRow[] };
}

export interface LabSplit {
  split: "train" | "valid" | "test";
  mean_per_bot_month_pct: number;
  ci95_pct: [number, number];
  months_positive: number;
  months: number;
}

export interface LabTemplate {
  template_id: string;
  preset_id: string;
  kind: "dca" | "grid";
  side: "long" | "short" | "neutral";
  universe: { rule: "topUsdtPerpsByQuoteVolume" | "btcOnly"; rank_from: number; top_n: number };
  claim: "proven" | "experimental";
  app_verdict: "presetReady" | "failed";
  fail_reasons: string[];
  splits: LabSplit[];
  test_bots: number;
  test_share_bots_positive: number;
  test_worst_bot_drawdown_pct: number;
  one_bot_worst_drawdown_pct: number | null;
  one_bot_median_drawdown_pct: number | null;
  test_sign_test_p: number;
  bonferroni_bar: number;
  liquidations: number;
  stopped_deals: number;
  max_deal_days: number;
  open_deals_at_data_end: number;
  recheck_partial_month_mean_pct: number | null;
}

export interface GhostTemplate {
  template_id: string;
  preset_id: string;
  universe: string;
  universe_month: string;
  symbols: string[];
  bots: number;
  capital_usdt: number;
  pnl_usdt: number;
  return_pct: number;
  cycles: number;
  open_cycles: number;
  win_rate_cycles: number | null;
  worst_dd_pct: number | null;
  median_dd_pct: number | null;
  stopped_share: number | null;
  liquidated_share: number | null;
}

export interface FarmTemplate {
  template_id: string;
  preset_id: string;
  name: string;
  kind: string;
  universe: string;
  shipped_verdict: string;
  bots: number;
  cycles_closed: number;
  cycles_open: number;
  allocated_usdt: number;
  realized_net_pnl_usdt: number;
  open_pnl_usdt: number;
  return_pct: number;
  max_drawdown_pct: number;
  worst_bot: { bot_id: string; symbol: string; return_pct: number } | null;
  rankable: boolean;
  not_rankable_reason: string | null;
}

export interface FarmRanking {
  status: "notAllowed" | "allowed";
  rule: string;
  min_days: number;
  min_closed_cycles_per_bot: number;
  min_closed_trades_signal_bot: number;
  engine_check_passed: boolean;
  reasons: string[];
  ranked: { unit: string; return_pct: number }[];
}

export interface Presets {
  generated_at: string;
  claim_rule: string;
  order: string;
  lab: Leaf & {
    source_sha256: string;
    capital_model: "botPerSymbolMonth";
    cost_model: { simulator: string; bar_interval: string; maker_fee_pct: number; taker_fee_pct: number; slippage_pct: number; fill_rule: string };
    majors_caveat: { template_id: "T01"; text: string; top_n: number; train_mean_pct: number; valid_mean_pct: number; test_mean_pct: number; worst_bot_drawdown_pct: number };
    recheck: { run_on: string; test_window_end: string; partial_month: string; partial_month_days: number; next_read: string; text: string };
    templates: LabTemplate[];
  };
  ghost: Leaf & {
    t0: string;
    data_end: string | null;
    days_running: number | null;
    min_days_for_ranking: number;
    ranking: "not_shown";
    method: string;
    templates: GhostTemplate[];
  };
  farm: Leaf & {
    file_generated_at?: string;
    t0?: string | null;
    days_running?: number;
    engine?: { app_version: string; fingerprint: string; farm_version: string };
    labels?: { mode: string; fills: string; text: string; funding: string; ranking: FarmRanking };
    templates?: FarmTemplate[];
    bot_count?: number;
    refused_count?: number;
  };
  similar_days: Leaf & { templates: [] };
}

export interface ImpactHorizon {
  status: "ok" | "measuring";
  n: number;
  needed?: number;
  median?: number;
  mean?: number;
  share_up?: number;
  base_mean?: number;
  base_share_up?: number;
  diff_ci?: [number, number];
  differs?: boolean;
}

export interface ImpactType {
  type: string;
  label: string;
  status: "ok" | "measuring" | "unavailable";
  reason?: string;
  horizons: Partial<Record<"1h" | "4h" | "24h", ImpactHorizon>>;
}

export interface NewsLine {
  group: string;
  text: string;
  source_name: string;
  title: string;
  url: string;
  published_at: string | null;
  impact_type: string;
}

export interface News extends Leaf {
  digest_status: string;
  item_count: number;
  lines_total: number;
  withheld_lines: number;
  lines: NewsLine[];
  impact: Leaf & {
    window_days: number;
    min_stories: number;
    baseline: string;
    units: string;
    disclosure: string;
    types: ImpactType[];
  };
  disclosure: string;
}

export interface ListingRow {
  exchange: string;
  market: string;
  symbol: string;
  name: string;
  pairs: string[];
  status: "upcoming" | "live" | "cancelled" | "announced";
  listing_time: string | null;
  postponed: boolean;
  cancelled_at: string | null;
  announcement: { source: string; title: string; url: string; published_at: string | null; fetched_at: string };
  reaction: null | {
    status: "ok" | "partial" | "pending" | "no_data";
    reason?: string;
    data_source?: string;
    ref_time: string | null;
    ret_5m_pct: number | null;
    ret_1h_pct: number | null;
    ret_24h_pct: number | null;
  };
}

export interface Listings extends Leaf {
  upcoming: ListingRow[];
  recent: ListingRow[];
  upcoming_total: number;
  recent_total: number;
  withheld_titles: number;
  sources: { id: string; status: string; last_ok_at: string | null; last_run_at: string | null }[];
  reaction_method: string;
  reaction_units: string;
  disclosure: string;
  reaction_summary: {
    exchange: string;
    market: string;
    listings: number;
    no_data: number;
    pending: number;
    horizons: { horizon: "5m" | "1h" | "24h"; n: number; mean_pct: number | null; median_pct: number | null; positive_rate_pct: number | null }[];
  }[];
}

export interface BeforeYouTrade {
  schema: "before-you-trade/1";
  generated_at: string;
  cache_ttl_seconds: number;
  disclosure: string;
  market: Market;
  presets: Presets;
  news: News;
  listings: Listings;
}

export interface BeforeYouTradeView {
  payload: BeforeYouTrade;
  /** When this desk received the payload (ms). */
  fetchedAtMs: number;
  /** The last fetch failed; this is the previous copy. */
  stale: boolean;
  error: string | null;
}

export function beforeYouTrade(refresh = false): Promise<BeforeYouTradeView> {
  if (inTauri()) return invoke<BeforeYouTradeView>("before_you_trade", { refresh });
  return devMock(
    () => import("./beforeYouTrade.mock"),
    async (m) => m.beforeYouTrade(refresh),
  );
}

/** Opens one of the payload's own source links (Rust refuses any other URL). */
export function openSourceLink(url: string): Promise<void> {
  if (inTauri()) return invoke<void>("byt_open_link", { url });
  return devMock(
    async () => null,
    async () => {
      window.open(url, "_blank", "noopener,noreferrer");
    },
  );
}
