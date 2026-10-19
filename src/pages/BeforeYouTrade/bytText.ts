/**
 * Every sentence of the "Before you trade" page that comes from the payload,
 * said in the reader's language. The payload's own text is English and is
 * NEVER shown: a sentence is built from its code plus its numbers.
 *
 * Where the code comes from:
 *  1. the payload's code field (`reason_code`, `*_code`; server schema_minor >= 1);
 *  2. otherwise (today's server, schema_minor absent) the code is recovered
 *     from the exact English sentence the server is known to send
 *     (LEGACY_* below, copied from the server's source), numbers included;
 *  3. a code (or sentence) this app does not know: a short generic line in
 *     the reader's language ("not available in this version").
 *
 * Third-party text (news digest lines, announcement titles) is reporting by
 * someone else, not ours to rewrite; the page labels its language instead.
 */

import type { TFunction } from "i18next";
import type { BeforeYouTrade, ChartRead, CodedReason, FarmRanking, LabTemplate, Presets } from "@/lib/ipc/beforeYouTrade/beforeYouTrade";
import { formatNumber, formatPercent, formatSignedPercent, formatTableTime } from "@/lib/format";

type Args = Record<string, unknown>;
export interface Coded {
  code: string;
  args?: Args;
}

// ---------------------------------------------------------------- reason codes

/** Reason codes this app can say (each has `byt.reason.<code>` in every locale). */
export const REASON_CODES = [
  "not_wired",
  "store_unavailable",
  "macro_warming_up",
  "macro_stale",
  "range_high_low_missing",
  "datahub_not_configured",
  "datahub_candles_unavailable",
  "datahub_too_few_candles",
  "funding_unavailable",
  "funding_stale",
  "funding_no_rows",
  "funding_partial",
  "chart_no_successful_read",
  "chart_read_too_old",
  "ghost_path_not_configured",
  "ghost_file_not_found",
  "ghost_file_unreadable",
  "ghost_invalid_json",
  "ghost_version_unknown",
  "ghost_no_updated_at",
  "ghost_stale",
  "ghost_no_templates",
  "ghost_younger_than_min",
  "farm_not_started",
  "farm_unreadable",
  "farm_invalid_json",
  "farm_schema_unknown",
  "farm_no_generated_at",
  "farm_stale",
  "farm_gate_not_passed",
  "similar_days_not_computed_v1",
  "news_collector_off",
  "news_no_digest",
  "news_digest_failed",
  "news_digest_old",
  "news_no_showable_line",
  "impact_job_off",
  "impact_not_run",
  "impact_stale",
  "impact_no_line_to_match",
  "impact_price_wrap_not_measured",
  "impact_type_not_in_summary",
  "impact_no_horizon",
  "impact_excluded",
  "listings_collector_off",
  "listings_no_channel_read",
  "listings_channels_never_read",
  "listings_channel_failed",
  "reaction_listing_cancelled",
  "reaction_no_venue_candles",
  "reaction_tokenized_spot",
  "reaction_no_usd_pair",
  "reaction_market_no_klines",
  "reaction_no_listing_time",
  "reaction_datahub_not_set",
  "reaction_symbol_unknown",
  "reaction_traded_before_listing",
  "reaction_no_candle_15m",
  "reaction_no_candle_yet",
  "reaction_datahub_error",
  "reaction_candles_missing",
  "reaction_some_horizons_missing",
  "reaction_other",
  "farm_rank_days_below_min",
  "farm_rank_data_gap",
  "farm_rank_engine_check_pending",
  "farm_rank_mixed_engines",
  "farm_rank_too_few_units",
  "farm_rank_other",
] as const;
const KNOWN_REASONS = new Set<string>(REASON_CODES);

const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
const exact = (text: string, code: string): [RegExp, string, (m: RegExpMatchArray) => Args] => [new RegExp(`^${esc(text)}$`), code, () => ({})];

/**
 * The server's English reasons before schema_minor 1 (beforeyoutrade/*.go,
 * listings/reaction.go, the farm export's ranking), mapped to their codes.
 */
const LEGACY_REASONS: [RegExp, string, (m: RegExpMatchArray) => Args][] = [
  [/ service not wired$/, "not_wired", () => ({})],
  [/ store unavailable$/, "store_unavailable", () => ({})],
  exact("btcmacro warming up or failing", "macro_warming_up"),
  [/^btcmacro state is (?:stale|\S+ old \(limit \S+\))$/, "macro_stale", () => ({})],
  exact("24h ticker high/low missing", "range_high_low_missing"),
  exact("DataHub not configured (DATAHUB_URL)", "datahub_not_configured"),
  exact("DataHub daily candles unavailable", "datahub_candles_unavailable"),
  [/^DataHub returned (\d+) closed daily candles; ATR\(14\) needs (\d+)$/, "datahub_too_few_candles", (m) => ({ got: Number(m[1]), need: Number(m[2]) })],
  exact("DataHub funding snapshot unavailable", "funding_unavailable"),
  exact("DataHub funding snapshot is stale", "funding_stale"),
  exact("DataHub funding snapshot has no BTCUSDT or ETHUSDT row", "funding_no_rows"),
  exact("one of BTCUSDT/ETHUSDT missing from the DataHub snapshot", "funding_partial"),
  exact("no successful chart read stored", "chart_no_successful_read"),
  [/^no successful chart read in the last (\d+) h \(newest (\S+)\)$/, "chart_read_too_old", (m) => ({ max_age_h: Number(m[1]), newest: m[2] })],
  exact("ghost track path not configured (EDGE_GHOST_JSON)", "ghost_path_not_configured"),
  [/^ghost track file not found/, "ghost_file_not_found", () => ({})],
  exact("ghost track file unreadable", "ghost_file_unreadable"),
  exact("ghost track file is not valid JSON", "ghost_invalid_json"),
  [/^ghost track version (-?\d+) not understood/, "ghost_version_unknown", (m) => ({ version: Number(m[1]) })],
  exact("ghost track file has no updated_at", "ghost_no_updated_at"),
  [/^ghost track last updated (\S+), more than (\d+) h ago$/, "ghost_stale", (m) => ({ updated_at: m[1], max_age_h: Number(m[2]) })],
  exact("ghost track file lists none of the twelve templates", "ghost_no_templates"),
  [/^forward track younger than its (\d+)-day minimum/, "ghost_younger_than_min", (m) => ({ min_days: Number(m[1]) })],
  [/^the bot farm has not started/, "farm_not_started", () => ({})],
  exact("farm export unreadable", "farm_unreadable"),
  exact("farm export is not valid JSON", "farm_invalid_json"),
  [/^farm export schema "(.*)" not understood/, "farm_schema_unknown", (m) => ({ schema: m[1] })],
  exact("farm export has no valid generated_at", "farm_no_generated_at"),
  [/^farm export last written (\S+), more than (\d+) min ago/, "farm_stale", (m) => ({ written_at: m[1], max_age_min: Number(m[2]) })],
  [/^the farm's pre-registered ranking gate is not passed yet/, "farm_gate_not_passed", () => ({})],
  [/^Not computed\. The lab figures are per bot-month/, "similar_days_not_computed_v1", () => ({})],
  exact("news collector is switched off", "news_collector_off"),
  exact("no digest stored yet", "news_no_digest"),
  [/^the latest digest run produced no summary \(status (.*)\)$/, "news_digest_failed", (m) => ({ status: m[1] })],
  [/^the latest digest is older than (\d+) h$/, "news_digest_old", (m) => ({ max_age_h: Number(m[1]) })],
  exact("the latest digest has no line that can be shown", "news_no_showable_line"),
  exact("news impact job is switched off", "impact_job_off"),
  exact("news impact has not run yet", "impact_not_run"),
  [/^news impact summary is older than (\d+) h$/, "impact_stale", (m) => ({ max_age_h: Number(m[1]) })],
  exact("no digest line to match", "impact_no_line_to_match"),
  [/^Headlines that describe a price move already made/, "impact_price_wrap_not_measured", () => ({})],
  exact("type not in the stored summary", "impact_type_not_in_summary"),
  exact("no horizon stored", "impact_no_horizon"),
  exact("listings collector is switched off", "listings_collector_off"),
  exact("no exchange channel has been read successfully yet", "listings_no_channel_read"),
  [/^never read successfully, so absent here: (.+)$/, "listings_channels_never_read", (m) => ({ channels: m[1].split(/,\s*/) })],
  [/^at least one exchange channel failed its last run/, "listings_channel_failed", () => ({})],
  exact("listing cancelled", "reaction_listing_cancelled"),
  [/^DataHub has no (\S+) candles$/, "reaction_no_venue_candles", (m) => ({ exchange: m[1] })],
  [/^tokenized-security spot pairs are not in DataHub/, "reaction_tokenized_spot", () => ({})],
  exact("no USDT/USDC/USD1 pair stated", "reaction_no_usd_pair"),
  [/^market (\S+) has no DataHub klines$/, "reaction_market_no_klines", (m) => ({ market: m[1] })],
  exact("no listing time stated in the announcement", "reaction_no_listing_time"),
  exact("DATAHUB_URL not set", "reaction_datahub_not_set"),
  [/^(\S+) not known to Binance (\S+) via DataHub$/, "reaction_symbol_unknown", (m) => ({ symbol: m[1], market: m[2] })],
  [/^(\S+) traded before the stated listing time/, "reaction_traded_before_listing", (m) => ({ symbol: m[1] })],
  exact("no candle within 15 min of the stated listing time", "reaction_no_candle_15m"),
  exact("no candle yet within 15 min of the stated listing time", "reaction_no_candle_yet"),
  [/^datahub: /, "reaction_datahub_error", () => ({})],
  exact("candles missing at every horizon", "reaction_candles_missing"),
  exact("some horizons had no candle", "reaction_some_horizons_missing"),
  [/^([0-9.]+) of ([0-9.]+) days since T0$/, "farm_rank_days_below_min", (m) => ({ days: Number(m[1]), min_days: Number(m[2]) })],
  [/^a data gap of (\d+) h/, "farm_rank_data_gap", (m) => ({ hours: Number(m[1]) })],
  [/^engine-vs-simulator check/, "farm_rank_engine_check_pending", () => ({})],
  [/more than one engine fingerprint/, "farm_rank_mixed_engines", () => ({})],
  [/^(\d+) unit\(s\) meet the closed-cycle minimum/, "farm_rank_too_few_units", (m) => ({ units: Number(m[1]) })],
];

/** The code of one of the server's known English reasons (null when unknown). */
export function legacyReasonCode(text: string | null | undefined): Coded | null {
  const s = (text ?? "").trim();
  if (!s) return null;
  for (const [rx, code, args] of LEGACY_REASONS) {
    const m = s.match(rx);
    if (m) return { code, args: args(m) };
  }
  return null;
}

const isIso = (v: unknown): v is string => typeof v === "string" && /^\d{4}-\d{2}-\d{2}T/.test(v);

/** Arguments as the reader reads them: numbers in their locale, times local, lists joined. */
function sayArgs(args: Args | undefined, locale: string, now: number): Args {
  const out: Args = {};
  for (const [k, v] of Object.entries(args ?? {})) {
    if (Array.isArray(v)) out[k] = v.join(", ");
    else if (typeof v === "number") out[k] = formatNumber(v, locale, { maximumFractionDigits: 2 });
    else if (isIso(v)) out[k] = formatTableTime(v, locale, now);
    else out[k] = v;
  }
  return out;
}

/** Says one coded reason; unknown codes give the generic line. */
export function sayCoded(t: TFunction, c: Coded, locale: string, now = Date.now()): string {
  if (!KNOWN_REASONS.has(c.code)) return t("byt.reason.unknown");
  return t(`byt.reason.${c.code}`, sayArgs(c.args, locale, now));
}

/** A reason-bearing object as the payload sends it. */
export interface HasReason {
  reason?: string | null;
  reason_code?: string;
  reason_args?: Record<string, unknown>;
}

/** The code of a reason: the payload's own, else the known English sentence's. */
export function reasonOf(x: HasReason): Coded | null {
  if (x.reason_code) return { code: x.reason_code, args: x.reason_args };
  if (!x.reason) return null;
  return legacyReasonCode(x.reason) ?? { code: "unknown" };
}

/** The reader's sentence for a reason, or null when there is none. */
export function reasonText(t: TFunction, x: HasReason, locale: string, now = Date.now()): string | null {
  const c = reasonOf(x);
  return c ? sayCoded(t, c, locale, now) : null;
}

/** The farm's ranking-gate reasons, coded. */
export function farmRankReasons(r: FarmRanking | undefined): Coded[] {
  if (!r) return [];
  if (r.reason_codes) return r.reason_codes.map((c: CodedReason) => ({ code: c.code, args: c.args }));
  return r.reasons.map((s) => legacyReasonCode(s) ?? { code: "unknown" });
}

// ---------------------------------------------------------------- fixed sentences

/** Fixed-sentence codes this app can say (`byt.text.<code>`). */
export const TEXT_CODES = [
  "fixed_template_order_v1",
  "partial_month_open_v1",
  "alt_long_guard_v1",
  "btcd_shield_48h_v1",
  "ema9_21_48h_v1",
  "atr14_1h_over_1_5pct_v1",
  "atr14_1d_closed_v1",
  "ghost_botsim_forward_v1",
  "farm_paper_simulated_v1",
  "farm_funding_settled_v1",
  "farm_funding_not_charged_v1",
  "news_third_party_v1",
  "news_impact_v1",
  "listings_v1",
  "listing_reaction_1m_v1",
  "chart_info_only_bar_not_met",
  "chart_bar_met",
  "chart_info_only_bar_open",
  // schema_minor 2
  "farm_signal_bots_paper_v1",
  "news_machine_translated_v1",
  "news_newer_run_failed_v1",
] as const;
const KNOWN_TEXT = new Set<string>(TEXT_CODES);

/** The server's fixed English sentences before schema_minor 1, by code. */
const LEGACY_TEXT: Record<string, string> = {
  "Templates are listed in fixed T01..T12 order in every source and never sorted by a result.": "fixed_template_order_v1",
  "The October re-run added a partial month in which every deal was still open and the mean was negative, so 'every test month positive' did not hold; the checks are read again on a full month.":
    "partial_month_open_v1",
  "New alt long signals are not published while the trusted BTC read has trend_state down together with one of: a 24h pulse at the low extreme, rising BTC dominance, alt_long_risk >= 0.60, trend_strength >= 0.35; after such a read the block holds for the sticky window unless trend_state turns up. Read here without changing it.":
    "alt_long_guard_v1",
  "trend code of the math-core BTCD shield on 48 BTCUSDT 1h candles": "btcd_shield_48h_v1",
  "EMA9 vs EMA21 on 48 BTCUSDT 1h candles; flat inside a 0.3% band": "ema9_21_48h_v1",
  "ATR(14) on BTCUSDT 1h over the mean close; high above 1.5%": "atr14_1h_over_1_5pct_v1",
  "mean true range of the last 14 closed daily candles; pct of the last close": "atr14_1d_closed_v1",
  "Lab simulator (botsim) forward from T0, template parameters as the Presets; budget 1000 per bot, 1x, monthly universes; open cycles marked at the last close; fees 0.05% + 0.02% slippage per market order, funding included.":
    "ghost_botsim_forward_v1",
  "Paper trading with simulated fills on the Aleph Edge engine. Not live results: real fills, slippage, fees and outages differ, and are expected to be worse.":
    "farm_paper_simulated_v1",
  "Futures DCA/Grid cycles are charged settled funding from DataHub.": "farm_funding_settled_v1",
  "Third-party reporting, not a Sentinel measurement. It is not used in any signal, veto or win rate.": "news_third_party_v1",
  "Measured history after past news, not a forecast. Each figure is BTC spot's move after past stories of one type, set against the same hours of the day in the same window. Headlines are typed by fixed keyword rules; price-wrap headlines written after a move are counted but never measured. Not used in any signal, veto or win rate.":
    "news_impact_v1",
  "Listing times and pairs are the exchanges' own announcements, linked. Reactions are measured from DataHub candles where they exist; no data is shown as no data. Not a signal.":
    "listings_v1",
  "ref = open of the first 1-minute candle at or after the stated listing time (first trade); ret_h = close of the 1-minute candle ending ref+h / ref - 1. Binance spot and USDT-M perps only (DataHub has no other venue's candles).":
    "listing_reaction_1m_v1",
  "Information only: the read is not wired into any veto because its pre-registered bar was not met.": "chart_info_only_bar_not_met",
  "The read met its pre-registered bar; see bar.verdict.": "chart_bar_met",
  "Information only: the read's pre-registered bar is still open.": "chart_info_only_bar_open",
};

const squash = (s: string) => s.replace(/\s+/g, " ").trim();
const LEGACY_TEXT_SQUASHED = new Map(Object.entries(LEGACY_TEXT).map(([k, v]) => [squash(k), v]));

/** The code of a fixed sentence: the payload's, else the known English sentence's. */
export function textCode(code: string | undefined, text: string | null | undefined): string | null {
  if (code) return code;
  if (!text) return null;
  return LEGACY_TEXT_SQUASHED.get(squash(text)) ?? (squash(text).startsWith("Futures DCA/Grid funding is NOT charged") ? "farm_funding_not_charged_v1" : "unknown");
}

/** Says a fixed sentence (null when the payload has none). */
export function sayText(t: TFunction, code: string | undefined, text: string | null | undefined, args: Args = {}): string | null {
  const c = textCode(code, text);
  if (!c) return null;
  if (!KNOWN_TEXT.has(c)) return t("byt.text.unknown");
  return t(`byt.text.${c}`, args);
}

// ---------------------------------------------------------------- sentences with numbers

const presetName = (t: TFunction, id: string) => t(`strategy.preset.names.${id}`, { defaultValue: id });
const listOf = (t: TFunction, items: string[]) => items.join(t("byt.text.listJoin"));

/** Proven template ids and the universe size the claim holds on. */
export function provenSet(p: Presets): { ids: string[]; top: number | null } {
  const proven = p.lab.templates.filter((x: LabTemplate) => x.claim === "proven");
  const ids = p.proven_ids ?? proven.map((x) => x.preset_id);
  const top = p.proven_top_n ?? (proven[0]?.universe.rule === "topUsdtPerpsByQuoteVolume" ? proven[0].universe.top_n : null);
  return { ids, top };
}

/** "proven = …": the claim rule. */
export function claimRuleText(t: TFunction, p: Presets): string {
  const code = p.claim_rule_code ?? (p.claim_rule?.startsWith("proven = ") ? "proven_majors_v1" : "unknown");
  const { ids, top } = provenSet(p);
  if (code !== "proven_majors_v1" || top === null || !ids.length) return t("byt.text.unknown");
  return t("byt.text.proven_majors_v1", { names: listOf(t, ids.map((id) => presetName(t, id))), top });
}

/** The top-30 caveat row under the template it concerns. */
export function majorsCaveatText(t: TFunction, p: Presets, locale: string): string {
  const c = p.lab.majors_caveat;
  const code = c.code ?? (c.text ? "top30_worst_bot_v1" : "unknown");
  if (code !== "top30_worst_bot_v1") return t("byt.text.unknown");
  const tpl = p.lab.templates.find((x) => x.template_id === c.template_id);
  return t("byt.text.top30_worst_bot_v1", {
    name: tpl ? presetName(t, tpl.preset_id) : c.template_id,
    top: c.top_n,
    train: formatSignedPercent(c.train_mean_pct, locale, 2),
    valid: formatSignedPercent(c.valid_mean_pct, locale, 2),
    test: formatSignedPercent(c.test_mean_pct, locale, 2),
    dd: formatPercent(Math.abs(c.worst_bot_drawdown_pct), locale, 1),
  });
}

/** The October re-run line. */
export function recheckText(t: TFunction, p: Presets, locale: string): string | null {
  const r = p.lab.recheck;
  return sayText(t, r.code, r.text, {
    month: r.partial_month,
    days: formatNumber(r.partial_month_days, locale, { maximumFractionDigits: 1 }),
    next: r.next_read,
  });
}

const BAR_RX_NOT_MET = /^Veto bar not met: ([0-9.]+)% at n=(\d+) \(needed ([0-9.]+)% and \+([0-9.]+) pp/;
const BAR_RX_MET = /^Veto bar met: ([0-9.]+)% at n=(\d+) \(\+(-?[0-9.]+) pp over rules, \+(-?[0-9.]+) pp over persistence\)/;
const BAR_RX_OPEN = /^Veto bar open: (\d+) of (\d+) graded/;

/** The chart read's pre-registered bar, said with its numbers. */
export function barText(t: TFunction, bar: NonNullable<ChartRead["bar"]>, locale: string): string {
  const p1 = (v: number | null | undefined) => (v === null || v === undefined ? "—" : formatPercent(v, locale, 1));
  const pp = (v: number | null | undefined) => (v === null || v === undefined ? "—" : formatNumber(v, locale, { maximumFractionDigits: 1, signDisplay: "exceptZero" } as Intl.NumberFormatOptions));
  let code = bar.code;
  let hit = bar.hit_pct ?? null;
  let n = bar.status === "open" ? (bar.graded_n ?? null) : bar.min_n;
  let needed = bar.needed_pct ?? null;
  let margin = bar.margin_pp ?? null;
  let mr = bar.margin_rule_pp ?? null;
  let mp = bar.margin_persist_pp ?? null;
  if (!code) {
    const v = bar.verdict ?? "";
    let m: RegExpMatchArray | null;
    if ((m = v.match(BAR_RX_NOT_MET))) {
      code = "veto_bar_not_met";
      [hit, n, needed, margin] = [Number(m[1]), Number(m[2]), Number(m[3]), Number(m[4])];
    } else if ((m = v.match(BAR_RX_MET))) {
      code = "veto_bar_met";
      [hit, n, mr, mp] = [Number(m[1]), Number(m[2]), Number(m[3]), Number(m[4])];
    } else if ((m = v.match(BAR_RX_OPEN))) {
      code = "veto_bar_open";
      n = Number(m[1]);
    } else {
      code = { open: "veto_bar_open", met: "veto_bar_met", not_met: "veto_bar_not_met", unavailable: "veto_bar_unavailable" }[bar.status] ?? "unknown";
    }
  }
  switch (code) {
    case "veto_bar_not_met":
      return hit === null
        ? t("byt.bar.notMetNoFigure", { n: bar.min_n })
        : t("byt.bar.notMet", { hit: p1(hit), n: n ?? bar.min_n, needed: formatPercent(needed ?? 60, locale, Number.isInteger(needed ?? 60) ? 0 : 1), margin: formatNumber(margin ?? 5, locale) });
    case "veto_bar_met":
      return hit === null ? t("byt.bar.metNoFigure", { n: bar.min_n }) : t("byt.bar.met", { hit: p1(hit), n: n ?? bar.min_n, rule: pp(mr), persist: pp(mp) });
    case "veto_bar_open":
      return t("byt.bar.open", { graded: n ?? "—", min: bar.min_n });
    case "veto_bar_unavailable":
      return t("byt.bar.unavailable");
    default:
      return t("byt.text.unknown");
  }
}

/** What the chart read is used for (the bar decides). */
export function chartUseText(t: TFunction, r: ChartRead): string | null {
  if (r.use_code || r.use) return sayText(t, r.use_code, r.use);
  return null;
}

/** The right panel's short facts (4), or one generic line when the disclosure is unknown. */
export function disclosureFacts(t: TFunction, b: BeforeYouTrade, locale: string): string[] {
  const code = b.disclosure_code ?? (b.disclosure?.startsWith("Measured history and third-party reporting") ? "byt_disclosure_v1" : "unknown");
  if (code !== "byt_disclosure_v1") return [t("byt.text.unknown")];
  const { ids, top } = provenSet(b.presets);
  const c = b.presets.lab.majors_caveat;
  const means = [c.train_mean_pct, c.valid_mean_pct, c.test_mean_pct];
  const facts = [
    `${t("byt.about.notAdvice")} ${t("byt.about.simulations")}`,
    ids.length && top !== null ? t("byt.about.proven", { names: listOf(t, ids.map((id) => presetName(t, id))), top }) : null,
    t("byt.about.wide", {
      top: c.top_n,
      losing: means.filter((m) => m < 0).length,
      splits: means.length,
      dd: formatPercent(Math.abs(c.worst_bot_drawdown_pct), locale, 1),
    }),
    t("byt.about.noSuggestion"),
  ];
  return facts.filter((x): x is string => !!x);
}

// ---------------------------------------------------------------- small vocabularies

/** A news impact type's name (proper vocabulary of the classifier). */
export const IMPACT_TYPES = [
  "macro_official",
  "fed_supervision",
  "sec_official",
  "exchange_security",
  "price_report",
  "macro_press",
  "etf_institutional",
  "stablecoin",
  "protocol_upgrade",
  "listing",
  "regulation_legal",
  "tokenization",
  "other",
] as const;
export function impactTypeName(t: TFunction, type: string): string {
  return (IMPACT_TYPES as readonly string[]).includes(type) ? t(`byt.impactTypes.${type}`) : t("byt.impactTypes.unknown");
}

/** The model's own confidence word, in the reader's language. */
export function confidenceText(t: TFunction, c: string): string {
  const k = c.trim().toLowerCase();
  return ["low", "medium", "high"].includes(k) ? t(`byt.chart.confidenceLevels.${k}`) : t("byt.chart.confidenceLevels.unknown");
}

/** A chart-read attempt status. */
export function attemptStatusText(t: TFunction, s: string): string {
  return ["error", "timeout", "skipped", "invalid"].includes(s) ? t(`byt.chart.attemptStatus.${s}`) : t("byt.chart.attemptStatus.other");
}

/** A listing channel's last-run status. */
export function feedStatusText(t: TFunction, s: string): string {
  return ["baseline", "http_error", "never_run", "parse_error"].includes(s) ? t(`byt.listings.feedStatus.${s}`) : t("byt.listings.feedStatus.other");
}

/** A leaf status word (unknown statuses get the generic word). */
export function statusText(t: TFunction, s: string): string {
  return ["ok", "measuring", "not_started", "unavailable"].includes(s) ? t(`byt.status.${s}`) : t("byt.status.unknown");
}

/** A farm signal bot's market. */
export function farmMarketText(t: TFunction, m: string): string {
  return m === "spot" || m === "futures" ? t(`byt.farm.signal.markets.${m}`) : t("byt.farm.signal.markets.other");
}

/** A farm bot's own state word (the farm's vocabulary). */
export function farmBotStateText(t: TFunction, s: string): string {
  return ["running", "dailyStop", "stopped", "dead"].includes(s) ? t(`byt.farm.signal.states.${s}`) : t("byt.farm.signal.states.other");
}

/**
 * A refusal reason code (the app's own skip reasons, `bots.skipReasons.*`)
 * with its count. The skip sentence's per-signal detail ("({{detail}} USDT)")
 * has no value in a count, so that part is left out.
 */
export function refusalText(t: TFunction, code: string, n: number): string {
  const key = `bots.skipReasons.${code}`;
  if (!/^[a-zA-Z]\w*$/.test(code) || !t(key, { defaultValue: "" })) return t("byt.farm.signal.refusalUnknown", { n });
  const mark = "@@detail@@";
  const reason = t(key, { detail: mark, count: n })
    .replace(/\s*\([^()]*@@detail@@[^()]*\)/g, "")
    .replace(/\s*[·:]?\s*@@detail@@/g, "")
    .trim();
  return t("byt.farm.signal.refusal", { reason, n });
}
