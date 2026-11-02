import { createContext, createEffect, createSignal, untrack, useContext, type Accessor } from 'solid-js'
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
  /** Adopts the server's language. Unlike `setLocale`, it is never written back. */
  applyServerLocale: (locale: Locale) => void
  tr: (noText: string, enText: string) => string
}

const fallbackI18n: I18nContextValue = {
  locale: () => defaultLocale,
  localeCode: () => localeCode(defaultLocale),
  localeName: () => 'Norsk',
  nextLocaleName: () => 'English',
  setLocale: () => undefined,
  toggleLocale: () => undefined,
  applyServerLocale: () => undefined,
  tr: (noText, enText) => pickLocaleText(defaultLocale, noText, enText),
}

export type LocaleChangeEvent = { locale: Locale; source: 'user' | 'server' }

const localeListeners = new Set<(event: LocaleChangeEvent) => void>()

/** Observes locale changes. `source: 'user'` is a toggle or Settings choice, `'server'` an adopted server value. */
export function subscribeLocaleChange(listener: (event: LocaleChangeEvent) => void): () => void {
  localeListeners.add(listener)
  return () => {
    localeListeners.delete(listener)
  }
}

function emitLocaleChange(event: LocaleChangeEvent): void {
  for (const listener of [...localeListeners]) {
    try {
      listener(event)
    } catch {
      // One failing observer must not break the others or the toggle.
    }
  }
}

const I18nContext = createContext<I18nContextValue>(fallbackI18n)

export function I18nProvider(props: { children: JSX.Element }) {
  const [locale, setLocaleSignal] = createSignal<Locale>(readInitialLocale())
  const setLocale = (nextLocale: Locale) => {
    const changed = untrack(locale) !== nextLocale
    setLocaleSignal(nextLocale)
    if (changed) emitLocaleChange({ locale: nextLocale, source: 'user' })
  }
  const applyServerLocale = (nextLocale: Locale) => {
    if (untrack(locale) === nextLocale) return
    setLocaleSignal(nextLocale)
    emitLocaleChange({ locale: nextLocale, source: 'server' })
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
    applyServerLocale,
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
export { LocalizedError, translateApiError } from '@/shared/i18n/errors'
