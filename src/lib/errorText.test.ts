import { beforeAll, describe, expect, it } from "vitest";
import i18n from "@/i18n";
import en from "@/i18n/locales/en/en.json";
import { localizeError } from "./errorText";

const enErrors = (en as unknown as { errors: Record<string, string> }).errors;

/**
 * Codes the Rust side returns through `localizeError` (vault, link, account,
 * bot desk, live trading). Each must have an `errors.*` sentence in en.json.
 */
const RUST_CODES = [
  "noAccessToken",
  "streamClosed",
  "vaultWrongPassword",
  "vaultPasswordTooShort",
  "vaultFileCorrupt",
  "vaultExists",
  "credentialExists",
  "credentialNotFound",
  "keyNoFutures",
  "keyRejected",
  "sentinelUnreachable",
  "serverRejected",
  "sessionExpired",
  "accountLookupFailed",
  "botDailyLossTripped",
  "botMaxPositionsRange",
  "botRiskRange",
  "botTakeProfitRange",
  "exchangeUnknown",
  "exchangeNoFutures",
  "liveBuildDisabled",
  "liveConfirmRequired",
  "liveFuturesBinanceOnly",
  "liveNeedsTradeKey",
  "linkSignInFirst",
  "positionNotFound",
  "closeNotConfirmed",
];

beforeAll(async () => {
  await i18n.changeLanguage("en");
});

describe("localizeError", () => {
  it.each(RUST_CODES)("%s has an en sentence and is localized", (code) => {
    expect(typeof enErrors[code], `errors.${code} missing in en.json`).toBe("string");
    const out = localizeError(code);
    expect(out).not.toBe(code);
    expect(out).toBe(i18n.t(`errors.${code}`, { detail: "" }));
  });

  it("a code without detail renders the sentence as is", () => {
    expect(localizeError("vaultWrongPassword")).toBe(enErrors.vaultWrongPassword);
  });

  it("splits code|detail and interpolates the detail", () => {
    expect(localizeError("vaultPasswordTooShort|12")).toBe("The vault password needs at least 12 characters.");
    expect(localizeError("exchangeUnknown|kraken")).toBe("Unknown exchange: kraken");
  });

  it("appends the detail in parentheses when the sentence has no {{detail}}", () => {
    expect(localizeError("keyRejected|-2015: Invalid API-key")).toBe(`${enErrors.keyRejected} (-2015: Invalid API-key)`);
  });

  it("keeps later pipes inside the detail and trims around them", () => {
    expect(localizeError(" serverRejected | a|b ")).toBe("Sentinel refused the request: a|b");
  });

  it("an unknown code, an empty code or a server's own wording comes back unchanged", () => {
    expect(localizeError("totallyUnknownCode")).toBe("totallyUnknownCode");
    expect(localizeError("totallyUnknownCode|detail")).toBe("totallyUnknownCode|detail");
    expect(localizeError("")).toBe("");
    expect(localizeError("|detail")).toBe("|detail");
    expect(localizeError("Connection reset by peer")).toBe("Connection reset by peer");
  });

  it("does not resolve a namespace object as a code", () => {
    // "errors" alone is not a leaf; nested namespaces are not under errors.*.
    expect(localizeError("historyEmpty")).toBe("historyEmpty");
  });

  it("follows the UI language", async () => {
    await i18n.changeLanguage("tr");
    try {
      const tr = localizeError("vaultWrongPassword");
      expect(tr).not.toBe(enErrors.vaultWrongPassword);
      expect(tr.length).toBeGreaterThan(0);
    } finally {
      await i18n.changeLanguage("en");
    }
  });
});
