import { describe, expect, it } from "vitest";
import i18n, { LANGUAGES } from "@/i18n";
import en from "@/i18n/locales/en/en.json";

// Every offered language must actually resolve to its own strings. pt-BR
// silently fell back to English (region stripped before the supported check).
describe("every app language resolves to its own translations", () => {
  for (const { code } of LANGUAGES) {
    it(code, async () => {
      await i18n.changeLanguage(code);
      expect(i18n.resolvedLanguage).toBe(code);
      if (code !== "en") expect(i18n.t("nav.dashboard")).not.toBe(en.nav.dashboard);
    });
  }

  it("a Portuguese browser without a region gets pt-BR, not English", async () => {
    await i18n.changeLanguage("pt-PT");
    expect(i18n.t("nav.dashboard")).toBe(i18n.getFixedT("pt-BR")("nav.dashboard"));
  });
});
