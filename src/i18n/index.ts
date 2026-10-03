import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

import en from "./locales/en/en.json";
import tr from "./locales/tr/tr.json";
import hi from "./locales/hi/hi.json";
import vi from "./locales/vi/vi.json";
import id from "./locales/id/id.json";
import ru from "./locales/ru/ru.json";
import ptBR from "./locales/pt-BR/pt-BR.json";
import es from "./locales/es/es.json";

/**
 * The supported languages: the languages of the 7 highest crypto-adoption
 * countries (Chainalysis Global Crypto Adoption Index) plus Turkish.
 * `endonym` is each language's name in its own script — always shown in the
 * language picker so a user finds their language without knowing English.
 */
export interface SupportedLanguage {
  code: string;
  endonym: string;
  /** BCP-47 tag for Intl number/currency/date formatting. */
  locale: string;
}

export const LANGUAGES: SupportedLanguage[] = [
  { code: "en", endonym: "English", locale: "en-US" },
  { code: "tr", endonym: "Türkçe", locale: "tr-TR" },
  { code: "hi", endonym: "हिन्दी", locale: "hi-IN" },
  { code: "vi", endonym: "Tiếng Việt", locale: "vi-VN" },
  { code: "id", endonym: "Bahasa Indonesia", locale: "id-ID" },
  { code: "ru", endonym: "Русский", locale: "ru-RU" },
  { code: "pt-BR", endonym: "Português (Brasil)", locale: "pt-BR" },
  { code: "es", endonym: "Español", locale: "es-ES" },
];

export const localeForLanguage = (code: string): string =>
  LANGUAGES.find((l) => l.code === code)?.locale ?? "en-US";

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      tr: { translation: tr },
      hi: { translation: hi },
      vi: { translation: vi },
      id: { translation: id },
      ru: { translation: ru },
      "pt-BR": { translation: ptBR },
      es: { translation: es },
    },
    // pt-BR is the only regional code. With nonExplicitSupportedLngs the
    // region is stripped before the supported check, so "pt" must be listed
    // too or every Brazilian (and pt-PT) user silently got English.
    fallbackLng: { pt: ["pt-BR", "en"], default: ["en"] },
    supportedLngs: [...LANGUAGES.map((l) => l.code), "pt"],
    nonExplicitSupportedLngs: true,
    interpolation: { escapeValue: false }, // React already escapes
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: "aleph-edge-lang",
      caches: ["localStorage"],
    },
  });

export default i18n;
