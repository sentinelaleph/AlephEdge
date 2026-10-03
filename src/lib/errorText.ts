import i18n from "@/i18n";

/**
 * Renders a Rust error/status code through `errors.*`.
 *
 * Rust returns stable camelCase codes (`vaultWrongPassword`), optionally with
 * a detail after a pipe (`binanceRefused|-2019: Margin is insufficient.`), the
 * same convention as `strategy.errors.*` and `backtest.errors.*`. A known
 * code becomes the user's language; the detail is passed as `{{detail}}` and
 * appended in parentheses when the sentence does not already carry it. An
 * unknown string (a server's own wording, a code added later) is returned as
 * it came, so a failure is never hidden behind a generic message.
 */
export function localizeError(raw: string): string {
  const [head, ...rest] = raw.split("|");
  const code = head.trim();
  const detail = rest.join("|").trim();
  const key = `errors.${code}`;
  if (!code || !i18n.exists(key)) return raw;
  const text = i18n.t(key, { detail });
  return detail && !text.includes(detail) ? `${text} (${detail})` : text;
}
