export type Locale = 'no' | 'en'

export const defaultLocale: Locale = 'no'
export const localeStorageKey = 'verevon.locale'

export function isLocale(value: unknown): value is Locale {
  return value === 'no' || value === 'en'
}

export function localeCode(locale: Locale): string {
  return locale === 'no' ? 'NO' : 'EN'
}

export function localeHtmlLang(locale: Locale): string {
  return locale === 'no' ? 'nb' : 'en'
}

export function localeIntl(locale: Locale): string {
  return locale === 'no' ? 'nb-NO' : 'en-US'
}

export function oppositeLocale(locale: Locale): Locale {
  return locale === 'no' ? 'en' : 'no'
}

export function pickLocaleText(locale: Locale, noText: string, enText: string): string {
  return locale === 'no' ? noText : enText
}

/**
 * The locale the user stored, or the default. Safe before any provider exists (the root error card reads it).
 * It lives here, not in the index barrel, so tests that mock '@/shared/i18n' with only useI18n cannot break it.
 */
export function readInitialLocale(): Locale {
  try {
    const stored = window.localStorage.getItem(localeStorageKey)
    if (isLocale(stored)) return stored
  } catch {
    return defaultLocale
  }

  return defaultLocale
}
