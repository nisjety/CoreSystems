import { createContext, createEffect, createSignal, useContext, type Accessor } from 'solid-js'
import type { JSX } from '@solidjs/web'
import {
  defaultLocale,
  isLocale,
  localeCode,
  localeHtmlLang,
  localeIntl,
  localeStorageKey,
  oppositeLocale,
  pickLocaleText,
  type Locale,
} from '@/shared/i18n/locales'

type I18nContextValue = {
  locale: Accessor<Locale>
  localeCode: Accessor<string>
  localeName: Accessor<string>
  nextLocaleName: Accessor<string>
  setLocale: (locale: Locale) => void
  toggleLocale: () => void
  tr: (noText: string, enText: string) => string
}

const fallbackI18n: I18nContextValue = {
  locale: () => defaultLocale,
  localeCode: () => localeCode(defaultLocale),
  localeName: () => 'Norsk',
  nextLocaleName: () => 'English',
  setLocale: () => undefined,
  toggleLocale: () => undefined,
  tr: (noText, enText) => pickLocaleText(defaultLocale, noText, enText),
}

const I18nContext = createContext<I18nContextValue>(fallbackI18n)

export function I18nProvider(props: { children: JSX.Element }) {
  const [locale, setLocaleSignal] = createSignal<Locale>(readInitialLocale())
  const setLocale = (nextLocale: Locale) => {
    setLocaleSignal(nextLocale)
  }

  createEffect(
    () => locale(),
    (currentLocale) => {
      document.documentElement.lang = localeHtmlLang(currentLocale)
      try {
        window.localStorage.setItem(localeStorageKey, currentLocale)
      } catch {
        // Storage may be unavailable in private mode or isolated test contexts.
      }
    },
  )

  const value: I18nContextValue = {
    locale,
    localeCode: () => localeCode(locale()),
    localeName: () => locale() === 'no' ? 'Norsk' : 'English',
    nextLocaleName: () => oppositeLocale(locale()) === 'no' ? 'Norsk' : 'English',
    setLocale,
    toggleLocale: () => setLocale(oppositeLocale(locale())),
    tr: (noText, enText) => pickLocaleText(locale(), noText, enText),
  }

  return (
    <I18nContext value={value}>
      {props.children}
    </I18nContext>
  )
}

export function useI18n(): I18nContextValue {
  return useContext(I18nContext)
}

export function localeDateTime(locale: Locale): string {
  return localeIntl(locale)
}

function readInitialLocale(): Locale {
  try {
    const stored = window.localStorage.getItem(localeStorageKey)
    if (isLocale(stored)) return stored
  } catch {
    return defaultLocale
  }

  return defaultLocale
}

export type { Locale }
export { localeStorageKey }
export { translateApiError } from '@/shared/i18n/errors'
