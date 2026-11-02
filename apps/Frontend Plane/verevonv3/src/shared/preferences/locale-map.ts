import type { Locale } from '@/shared/i18n/locales'

/**
 * Maps the server's stored `language` tag onto the two locales the SPA ships.
 * `nb*`, `nn*` and `no*` are Norwegian, `en*` is English, and anything else
 * (`fr-FR`, `de-DE`, an empty value) maps to nothing, so the SPA leaves that
 * server value untouched instead of overwriting it.
 */
export function localeFromLanguage(language: string | undefined | null): Locale | undefined {
  const tag = (language ?? '').trim().toLowerCase()
  if (!tag) return undefined
  const primary = tag.split(/[-_]/)[0] ?? ''
  if (primary === 'nb' || primary === 'nn' || primary === 'no') return 'no'
  if (primary === 'en') return 'en'
  return undefined
}

/** The language tag written to the server for a SPA locale. */
export function languageFromLocale(locale: Locale): 'nb-NO' | 'en-US' {
  return locale === 'no' ? 'nb-NO' : 'en-US'
}
