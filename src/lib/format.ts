/**
 * Locale-aware formatting. All number/currency/percent/date rendering goes
 * through here so an Indonesian user sees "1.234,50" and a US user "1,234.50"
 * from the same value — never hand-rolled string math.
 *
 * Which helper for which value:
 *   price (entry, TP, SL, mark)   formatPrice        "86,188" · "1.50" · "0.000004912"
 *   money amount (budget, size)   formatUsdt         "1,000.00 USDT"
 *   money result (P&L)            formatPnl / formatSignedUsdt + pnlTone
 *   percent level (win rate)      formatPercent      "54.80%"
 *   percent change (P&L, ROI)     formatSignedPercent + pnlTone
 *   elapsed time (last signal)    formatAge          "45 sec" · "2 min" · "1 hr"
 *   latency                       formatLatency      "74 ms"
 *   timestamp in a table          formatTableTime    "10:55" today, "Oct 1, 22:51" otherwise
 *
 * A value that rounds to zero at the shown precision is zero: no sign, and
 * pnlTone() calls it "flat" so it is drawn in the neutral colour.
 */

/** Placeholder for a value that does not exist (not a number, not sourced). */
export const NO_VALUE = "—";

export function formatNumber(
  value: number,
  locale: string,
  opts?: Intl.NumberFormatOptions,
): string {
  if (!Number.isFinite(value)) return NO_VALUE;
  return new Intl.NumberFormat(locale, opts).format(value);
}

export function formatCurrency(
  value: number,
  locale: string,
  currency = "USD",
): string {
  if (!Number.isFinite(value)) return NO_VALUE;
  return new Intl.NumberFormat(locale, {
    style: "currency",
    currency,
    maximumFractionDigits: value >= 1000 ? 0 : 2,
  }).format(value);
}

/** The value as it will be shown at `digits` decimals; -0 becomes 0. */
export function roundTo(value: number, digits: number): number {
  const f = 10 ** digits;
  const r = Math.round(value * f) / f;
  return r === 0 ? 0 : r;
}

export type PnlTone = "up" | "down" | "flat";

/**
 * Direction colour of a signed result at the precision it is displayed:
 * -0.004 shown as "0.00" is flat, not red.
 */
export function pnlTone(value: number | null | undefined, digits = 2): PnlTone {
  if (value == null || !Number.isFinite(value)) return "flat";
  const r = roundTo(value, digits);
  return r > 0 ? "up" : r < 0 ? "down" : "flat";
}

/**
 * The `data-tone` a table cell / KpiTile / FactList takes for a signed
 * result: "up" | "down", or undefined for flat (inherits the neutral text).
 */
export function pnlToneAttr(value: number | null | undefined, digits = 2): "up" | "down" | undefined {
  const t = pnlTone(value, digits);
  return t === "flat" ? undefined : t;
}

function signed(value: number, locale: string, digits: number): string {
  const r = roundTo(value, digits);
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
    signDisplay: r === 0 ? "never" : "always",
  }).format(r);
}

/** Signed percent, e.g. "+1.72%" / "-0.81%" / "0.00%". */
export function formatSignedPercent(
  value: number,
  locale: string,
  digits = 2,
): string {
  if (!Number.isFinite(value)) return NO_VALUE;
  return `${signed(value, locale, digits)}%`;
}

/** Unsigned percent with fixed decimals, e.g. "54.80%". Input is already in percent. */
export function formatPercent(value: number, locale: string, digits = 2): string {
  if (!Number.isFinite(value)) return NO_VALUE;
  const r = roundTo(value, digits);
  return `${new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(r)}%`;
}

/** A USDT amount: "1,000.00 USDT". `digits` 0 for whole budgets. */
export function formatUsdt(value: number, locale: string, digits = 2): string {
  if (!Number.isFinite(value)) return NO_VALUE;
  const r = roundTo(value, digits);
  return `${new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(r)} USDT`;
}

/** A signed USDT result: "+12.40 USDT" / "-0.04 USDT" / "0.00 USDT". */
export function formatSignedUsdt(value: number, locale: string, digits = 2): string {
  if (!Number.isFinite(value)) return NO_VALUE;
  return `${signed(value, locale, digits)} USDT`;
}

/** A P&L figure and its colour, rounded once so the two always agree. */
export function formatPnl(
  value: number,
  locale: string,
  opts: { digits?: number; unit?: "usdt" | "percent" | "none" } = {},
): { text: string; tone: PnlTone } {
  const digits = opts.digits ?? 2;
  if (!Number.isFinite(value)) return { text: NO_VALUE, tone: "flat" };
  const body = signed(value, locale, digits);
  const unit = opts.unit ?? "usdt";
  const text = unit === "usdt" ? `${body} USDT` : unit === "percent" ? `${body}%` : body;
  return { text, tone: pnlTone(value, digits) };
}

/**
 * Countdown to a known future event. With a locale the units are the
 * language's own short units ("73 sa 45 dk", "73 h 45 min"); without one the
 * compact "4h12m" form.
 */
export function formatDuration(ms: number, locale?: string): string {
  const totalMinutes = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 60_000) : 0;
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (!locale) return hours === 0 ? `${minutes}m` : `${hours}h${minutes}m`;
  if (hours === 0) return unit(locale, "minute", minutes);
  return `${unit(locale, "hour", hours)} ${unit(locale, "minute", minutes)}`;
}

/**
 * Decimals a sub-1 price needs for `sig` significant digits:
 * 0.0511 -> 5 ("0.05110"), 0.0000049 -> 9 ("0.000004900").
 */
export function priceDecimals(value: number, sig = 4): number {
  const a = Math.abs(value);
  if (!Number.isFinite(a) || a === 0) return 2;
  if (a >= 1000) return 0;
  if (a >= 1) return 2;
  const leadingZeros = Math.floor(-Math.log10(a)); // 0.05 -> 1, 0.0000049 -> 5
  return Math.min(16, Math.max(4, leadingZeros + sig));
}

/**
 * A price with precision for its magnitude: whole numbers from 1,000, two
 * decimals from 1, and at least 4 significant digits below 1, so a PEPE
 * entry of 0.0000049123 never collapses into its TP/SL neighbours.
 */
export function formatPrice(value: number, locale: string): string {
  if (!Number.isFinite(value)) return NO_VALUE;
  const digits = priceDecimals(value);
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}

function unit(locale: string, u: "second" | "minute" | "hour" | "day" | "millisecond", n: number): string {
  return new Intl.NumberFormat(locale, { style: "unit", unit: u, unitDisplay: "short" }).format(n);
}

/**
 * Elapsed time in the largest whole unit: "45 sec", "2 min", "1 hr", "3 days"
 * (the locale's own short units). The one age format of the app: the label
 * next to it says what the age is of ("Last signal").
 */
export function formatAge(secs: number, locale: string): string {
  if (!Number.isFinite(secs)) return NO_VALUE;
  const s = Math.max(0, Math.floor(secs));
  if (s < 60) return unit(locale, "second", s);
  if (s < 3600) return unit(locale, "minute", Math.floor(s / 60));
  if (s < 86_400) return unit(locale, "hour", Math.floor(s / 3600));
  return unit(locale, "day", Math.floor(s / 86_400));
}

/** Latency: "74 ms", or "1.2 s" from one second. */
export function formatLatency(ms: number, locale: string): string {
  if (!Number.isFinite(ms)) return NO_VALUE;
  if (ms >= 1000) {
    return new Intl.NumberFormat(locale, {
      style: "unit",
      unit: "second",
      unitDisplay: "short",
      maximumFractionDigits: 1,
    }).format(ms / 1000);
  }
  return unit(locale, "millisecond", Math.round(ms));
}

function sameLocalDay(a: Date, b: Date): boolean {
  return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();
}

/**
 * A timestamp in a table cell: time only when it is today, date + time
 * otherwise, the year only when it is not this year. Accepts UNIX ms, an ISO
 * string or a Date.
 */
export function formatTableTime(at: number | string | Date, locale: string, now: number | Date = Date.now()): string {
  const d = at instanceof Date ? at : new Date(at);
  if (Number.isNaN(d.getTime())) return NO_VALUE;
  const n = now instanceof Date ? now : new Date(now);
  const time: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
  if (sameLocalDay(d, n)) return new Intl.DateTimeFormat(locale, time).format(d);
  const date: Intl.DateTimeFormatOptions = { month: "short", day: "numeric", ...time };
  if (d.getFullYear() !== n.getFullYear()) date.year = "numeric";
  return new Intl.DateTimeFormat(locale, date).format(d);
}
