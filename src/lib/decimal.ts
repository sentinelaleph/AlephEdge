import i18n, { localeForLanguage } from "@/i18n";

/**
 * Locale-aware decimal entry for plain text inputs.
 *
 * `<input type="number">` is not usable here: in a "," decimal locale
 * (tr, es, pt-BR, ru, id, vi, en-GB keyboards) the browser drops or
 * reinterprets the comma, so "1,5" arrives as 15 or as nothing. Fields take
 * text and go through `parseDecimal`; values written back into a field go
 * through `formatDecimalInput` so a round trip never changes the number.
 */

function locale(): string {
  return localeForLanguage(i18n.resolvedLanguage ?? "en");
}

/** The grouping separator of the UI language ("," in en, "." in tr). */
function groupSeparator(): string {
  return new Intl.NumberFormat(locale()).formatToParts(1_000_000).find((p) => p.type === "group")?.value ?? ",";
}

/**
 * Accepts "." or "," as the decimal mark, and the UI language's own
 * thousands grouping ("1,000.5" in en, "1.000,5" in tr). One separator
 * followed by exactly three digits ("1,000" / "1.000") is read as grouping
 * when it is that language's grouping mark: replacing the first comma with a
 * dot used to read an English "1,000" budget as 1. Anything else that is not
 * a plain decimal (two marks of one kind, letters, hex) is not a number.
 * Empty text is NaN: callers decide what "empty" means, explicitly.
 */
export function parseDecimal(text: string): number {
  const s = text.trim().replace(/[\s  ]/g, "");
  if (s === "") return Number.NaN;
  const g = groupSeparator();
  const d = g === "," ? "." : ",";
  const esc = (c: string) => (c === "." ? "\\." : c);
  const grouped = new RegExp(`^-?[1-9]\\d{0,2}(?:${esc(g)}\\d{3})+(?:${esc(d)}\\d+)?$`);
  if (grouped.test(s)) return Number(s.split(g).join("").replace(d, "."));
  if (s.includes(g) && ownGrouping(s, g, d)) return Number(s.split(g).join("").replace(d, "."));
  if (!/^-?(?:\d+(?:[.,]\d*)?|[.,]\d+)$/.test(s)) return Number.NaN;
  return Number(s.replace(",", "."));
}

/**
 * The language's own Intl grouping when it is not plain groups of three:
 * hi-IN writes 1000000 as "10,00,000". Accepted only when the integer part is
 * exactly what Intl would print for it, so a stray separator is still rejected.
 */
function ownGrouping(s: string, g: string, d: string): boolean {
  const neg = s.startsWith("-");
  const body = neg ? s.slice(1) : s;
  const at = body.indexOf(d);
  const int = at >= 0 ? body.slice(0, at) : body;
  const frac = at >= 0 ? body.slice(at + 1) : "";
  if (!/^[1-9]\d*$/.test(int.split(g).join("")) || (at >= 0 && !/^\d+$/.test(frac))) return false;
  const shown = new Intl.NumberFormat(locale(), { useGrouping: true }).format(BigInt(int.split(g).join("")));
  return shown === int;
}

/** `String(n)` without exponent form: "1.23e-7" -> "0.000000123", "1e+21" -> "1000…0". */
function plainDecimal(n: number): string {
  const s = String(n);
  const m = /^(-?)(\d+)(?:\.(\d+))?e([+-]\d+)$/i.exec(s);
  if (!m) return s;
  const [, sign, intDigits, fracDigits = "", expText] = m;
  const digits = intDigits + fracDigits;
  const point = intDigits.length + Number(expText);
  if (point <= 0) return `${sign}0.${"0".repeat(-point)}${digits}`;
  if (point >= digits.length) return sign + digits + "0".repeat(point - digits.length);
  return `${sign}${digits.slice(0, point)}.${digits.slice(point)}`;
}

/**
 * A number as editable field text: no grouping, the UI language's decimal
 * mark. `String(1.125)` in tr would read back as 1125 ("." groups there);
 * "1,125" reads back as 1.125 in every language.
 */
export function formatDecimalInput(n: number | null | undefined): string {
  if (n == null || !Number.isFinite(n)) return "";
  // Exponent form ("1.23e-7", a sub-micro price) would not parse back.
  const s = plainDecimal(n);
  return groupSeparator() === "," ? s : s.replace(".", ",");
}
