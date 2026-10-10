import { describe, expect, it } from "vitest";

/**
 * Audit 2026-10-08, ml_claims 1: Sentinel's signal score was shown as
 * "Evidence" ("Evidence-first signals", "Min evidence score") although the
 * project measured it as not separating winners from losers (AUC 0.496).
 * It is labelled as a plain score now, and the honest result is stated where
 * the score is used. This test keeps the old wording from coming back in any
 * language.
 */
const LOCALES = import.meta.glob("/src/i18n/locales/*/*.json", { import: "default", eager: true }) as Record<
  string,
  Record<string, unknown>
>;

/** The evidence word of each locale's old copy. */
const EVIDENCE_WORDS = [/evidence/i, /kanıt/i, /evidencia/i, /evidência/i, /bukti/i, /доказательн/i, /bằng chứng/i, /प्रमाण/, /साक्ष्य/];

const SCORE_KEYS = [
  "bots.principles.items.evidence.title",
  "bots.principles.items.evidence.body",
  "signalsPage.evidence",
  "filters.minEvidence",
  "bots.minConfidence",
  "bots.skipReasons.belowConfidence",
  "signalDesk.evidenceScoreTooltip",
];

function lookup(tree: Record<string, unknown>, key: string): unknown {
  return key.split(".").reduce<unknown>((o, k) => (o && typeof o === "object" ? (o as Record<string, unknown>)[k] : undefined), tree);
}

describe("the Sentinel score is not called evidence", () => {
  it("sees all 8 locales", () => {
    expect(Object.keys(LOCALES).length).toBe(8);
  });

  for (const [file, tree] of Object.entries(LOCALES)) {
    it(file, () => {
      for (const key of SCORE_KEYS) {
        const text = lookup(tree, key);
        expect(typeof text, `${file} ${key}`).toBe("string");
        for (const word of EVIDENCE_WORDS) expect(text as string, `${file} ${key}`).not.toMatch(word);
      }
      // The tested result travels with the score wherever it is explained.
      for (const key of ["signalDesk.evidenceScoreTooltip", "bots.minConfidenceHint", "bots.principles.items.evidence.body"]) {
        expect(lookup(tree, key) as string, `${file} ${key}`).toContain("AUC 0.496");
      }
    });
  }
});
