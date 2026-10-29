export const locales = ["nb", "en"] as const;

export type Locale = (typeof locales)[number];

export const defaultLocale: Locale = "nb";

/** Values for the `lang` attribute, hreflang and schema.org `inLanguage`. */
export const htmlLang: Record<Locale, string> = {
  nb: "nb-NO",
  en: "en",
};

export const openGraphLocale: Record<Locale, string> = {
  nb: "nb_NO",
  en: "en_GB",
};

/** Content that exists in every locale, picked with `localized[locale]`. */
export type Localized<T> = Record<Locale, T>;
