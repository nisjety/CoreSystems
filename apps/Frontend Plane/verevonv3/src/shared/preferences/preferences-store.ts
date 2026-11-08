import { createSignal } from 'solid-js'
import { getPreferences, updatePreferences, type UserPreferences } from '@/shared/api/settings-client'
import { isValidTimeZone, type FormatPrefs } from '@/shared/i18n/format'
import { isLocale, localeStorageKey, type Locale } from '@/shared/i18n/locales'
import { languageFromLocale, localeFromLanguage } from './locale-map'

/**
 * Source of truth: `preferences` (Control `user_settings`) governs UI
 * formatting: language, time zone, date format, time format and density.
 * `profile.timezone` (Control `user_profiles`) is a directory fact shown to
 * colleagues and is only a fallback, never an input to this store.
 *
 * Everything here is fail-open. A failed read keeps the cache and defaults, a
 * failed write leaves a marker for the next boot, and nothing blocks render.
 */

export const LOCALE_SYNCED_KEY = 'verevon.locale.synced.v1'
export const LOCALE_DIRTY_KEY = 'verevon.locale.dirty'
export const REREAD_AFTER_HIDDEN_MS = 60_000
export const LOCALE_WRITE_DEBOUNCE_MS = 500

const [preferences, setPreferences] = createSignal<UserPreferences | undefined>(undefined)

/** The last preferences read from or written to the server, or undefined before the first read. */
export function currentPreferences(): UserPreferences | undefined {
  return preferences()
}

export function receivePreferences(next: UserPreferences): void {
  setPreferences(next)
}

export function clearPreferences(): void {
  setPreferences(undefined)
}

/**
 * Format preferences for `useFormat()`. A field is included only when the
 * server marks it explicit, so an unset `MM/DD/YYYY` default never overrides a
 * Norwegian reader's locale. Without the `explicit` block (an older server)
 * nothing is applied.
 */
export function formatPrefsFrom(prefs: UserPreferences | undefined): FormatPrefs | undefined {
  const explicit = prefs?.explicit
  if (!prefs || !explicit) return undefined
  const out: FormatPrefs = {}
  if (explicit.timezone && prefs.timezone && isValidTimeZone(prefs.timezone)) out.timeZone = prefs.timezone
  if (explicit.dateFormat && prefs.dateFormat && prefs.dateFormat !== 'auto') out.dateFormat = prefs.dateFormat
  if (explicit.timeFormat && prefs.timeFormat && prefs.timeFormat !== 'auto') out.timeFormat = prefs.timeFormat
  return Object.keys(out).length > 0 ? out : undefined
}

export function densityFrom(prefs: UserPreferences | undefined): 'compact' | 'comfortable' {
  return prefs?.compactMode === true ? 'compact' : 'comfortable'
}

export function applyDensity(density: 'compact' | 'comfortable'): void {
  try {
    document.documentElement.dataset.density = density
  } catch {
    // No DOM (tests, SSR): nothing to mark.
  }
}

// ---- locale precedence ----------------------------------------------------

export type LocaleDecision =
  | { kind: 'none' }
  /** Local value wins: PATCH it to the server (adopt-once, or a dirty retry). */
  | { kind: 'push-local'; locale: Locale }
  /** Server value wins: apply it to the UI without writing it back. */
  | { kind: 'apply-server'; locale: Locale }

export type LocaleFacts = {
  /** The locale in use now. */
  current: Locale
  /** A locale was cached in this browser before this boot. */
  hadLocalValue: boolean
  /** The adopt-once marker is set. */
  synced: boolean
  /** A previous write-through failed or never finished. */
  dirty: boolean
  prefs: UserPreferences
}

/**
 * Locale precedence, in order:
 * 1. a dirty local value wins and is retried;
 * 2. adopt-once: a local value present before the marker exists wins and is PATCHed;
 * 3. otherwise the server wins when it holds an explicit language that maps
 *    to a locale different from the current one;
 * 4. an unmapped server language (`fr-FR`) is left untouched.
 */
export function decideLocale(facts: LocaleFacts): LocaleDecision {
  if (facts.dirty && facts.hadLocalValue) return { kind: 'push-local', locale: facts.current }
  if (!facts.synced && facts.hadLocalValue) return { kind: 'push-local', locale: facts.current }
  const server = localeFromLanguage(facts.prefs.language)
  if (facts.prefs.explicit?.language && server && server !== facts.current) {
    return { kind: 'apply-server', locale: server }
  }
  return { kind: 'none' }
}

function readFlag(key: string): boolean {
  try {
    return window.localStorage.getItem(key) !== null
  } catch {
    return false
  }
}

function writeFlag(key: string, on: boolean): void {
  try {
    if (on) window.localStorage.setItem(key, '1')
    else window.localStorage.removeItem(key)
  } catch {
    // Storage may be unavailable; the next boot simply re-adopts.
  }
}

function readCachedLocale(): Locale | undefined {
  try {
    const stored = window.localStorage.getItem(localeStorageKey)
    return isLocale(stored) ? stored : undefined
  } catch {
    return undefined
  }
}

// The i18n provider writes the default locale into storage on mount, so whether
// a locale was cached BEFORE this boot has to be captured when this module loads.
const hadLocalLocaleAtBoot = readCachedLocale() !== undefined

// ---- controller -------------------------------------------------------------

export type PreferencesControllerDeps = {
  getLocale: () => Locale
  applyServerLocale: (locale: Locale) => void
  /** Registers a listener for locale changes; only `source: 'user'` is written through. */
  subscribeLocaleChange: (listener: (event: { locale: Locale; source: 'user' | 'server' }) => void) => () => void
}

/**
 * Starts the bootstrap: one read now, a re-read after the tab was hidden for at
 * least 60 s, and write-through of user language toggles (debounced 500 ms).
 * Returns a disposer.
 */
export function startPreferencesBootstrap(deps: PreferencesControllerDeps): () => void {
  let disposed = false
  let hiddenAt: number | undefined
  let writeTimer: ReturnType<typeof setTimeout> | undefined
  let firstBoot = true

  const pushLocale = async (locale: Locale): Promise<void> => {
    writeFlag(LOCALE_DIRTY_KEY, true)
    try {
      const saved = await updatePreferences({ language: languageFromLocale(locale) })
      if (disposed) return
      receivePreferences(saved)
      writeFlag(LOCALE_DIRTY_KEY, false)
      writeFlag(LOCALE_SYNCED_KEY, true)
    } catch {
      // Left dirty: the local value wins and is retried at the next boot.
    }
  }

  const load = async (): Promise<void> => {
    let prefs: UserPreferences
    try {
      prefs = await getPreferences()
    } catch {
      return
    }
    if (disposed) return
    receivePreferences(prefs)
    if (!firstBoot) return
    firstBoot = false
    const decision = decideLocale({
      current: deps.getLocale(),
      hadLocalValue: hadLocalLocaleAtBoot,
      synced: readFlag(LOCALE_SYNCED_KEY),
      dirty: readFlag(LOCALE_DIRTY_KEY),
      prefs,
    })
    if (decision.kind === 'push-local') void pushLocale(decision.locale)
    else if (decision.kind === 'apply-server') {
      deps.applyServerLocale(decision.locale)
      writeFlag(LOCALE_SYNCED_KEY, true)
    }
  }

  const unsubscribe = deps.subscribeLocaleChange((event) => {
    if (event.source !== 'user') return
    writeFlag(LOCALE_DIRTY_KEY, true)
    if (writeTimer) clearTimeout(writeTimer)
    writeTimer = setTimeout(() => {
      writeTimer = undefined
      void pushLocale(event.locale)
    }, LOCALE_WRITE_DEBOUNCE_MS)
  })

  const onVisibility = () => {
    if (document.visibilityState === 'hidden') {
      hiddenAt = Date.now()
      return
    }
    const hiddenFor = hiddenAt === undefined ? 0 : Date.now() - hiddenAt
    hiddenAt = undefined
    if (hiddenFor >= REREAD_AFTER_HIDDEN_MS) void load()
  }
  document.addEventListener('visibilitychange', onVisibility)

  void load()

  return () => {
    disposed = true
    if (writeTimer) clearTimeout(writeTimer)
    unsubscribe()
    document.removeEventListener('visibilitychange', onVisibility)
  }
}
