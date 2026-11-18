// @vitest-environment jsdom

import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'
import {
  LOCALE_DIRTY_KEY,
  LOCALE_SYNCED_KEY,
  LOCALE_WRITE_DEBOUNCE_MS,
  REREAD_AFTER_HIDDEN_MS,
  decideLocale,
  formatPrefsFrom,
  type LocaleFacts,
} from '@/shared/preferences/preferences-store'

// WS16-S3: how the preferences bootstrap picks a locale and when it reads and writes. decideLocale and
// formatPrefsFrom are pure. The bootstrap runs against a stubbed gateway; each test loads a fresh copy
// of the store, so the locale cached before this boot is read from that test's own storage.

const NOT_EXPLICIT = { language: false, timezone: false, dateFormat: false, timeFormat: false, compactMode: false }

type Locale = 'no' | 'en'
type LocaleEvent = { locale: Locale; source: 'user' | 'server' }

const stops: Array<() => void> = []

afterEach(() => {
  for (const stop of stops.splice(0)) stop()
  vi.useRealTimers()
  vi.unstubAllGlobals()
  delete (document as unknown as { visibilityState?: DocumentVisibilityState }).visibilityState
})

function facts(overrides: Partial<LocaleFacts>): LocaleFacts {
  return {
    current: 'no',
    hadLocalValue: false,
    synced: false,
    dirty: false,
    prefs: { language: 'nb-NO', explicit: { ...NOT_EXPLICIT, language: true } },
    ...overrides,
  }
}

describe('decideLocale', () => {
  it('lets a dirty local value win and retries it', () => {
    expect(decideLocale(facts({ current: 'en', hadLocalValue: true, synced: true, dirty: true })))
      .toEqual({ kind: 'push-local', locale: 'en' })
  })

  it('lets a locale cached before the adopt-once marker win over an explicit server language', () => {
    expect(decideLocale(facts({ current: 'en', hadLocalValue: true, synced: false })))
      .toEqual({ kind: 'push-local', locale: 'en' })
  })

  it('applies an explicit server language that maps to another locale', () => {
    expect(decideLocale(facts({
      current: 'no',
      hadLocalValue: true,
      synced: true,
      prefs: { language: 'en-US', explicit: { ...NOT_EXPLICIT, language: true } },
    }))).toEqual({ kind: 'apply-server', locale: 'en' })
  })

  it('does nothing when the explicit server language is already the locale in use', () => {
    expect(decideLocale(facts({
      current: 'en',
      synced: true,
      prefs: { language: 'en-US', explicit: { ...NOT_EXPLICIT, language: true } },
    }))).toEqual({ kind: 'none' })
  })

  it('ignores a server language the person never chose', () => {
    expect(decideLocale(facts({
      current: 'no',
      synced: true,
      prefs: { language: 'en-US', explicit: NOT_EXPLICIT },
    }))).toEqual({ kind: 'none' })
  })

  it('leaves an unmapped server language such as fr-FR untouched', () => {
    expect(decideLocale(facts({
      current: 'no',
      synced: true,
      prefs: { language: 'fr-FR', explicit: { ...NOT_EXPLICIT, language: true } },
    }))).toEqual({ kind: 'none' })
  })
})

describe('formatPrefsFrom', () => {
  it('is undefined for an older server with no explicit block, and when nothing is explicit', () => {
    expect(formatPrefsFrom(undefined)).toBeUndefined()
    expect(formatPrefsFrom({ dateFormat: 'DD/MM/YYYY', timeFormat: '24h', timezone: 'Europe/Oslo' })).toBeUndefined()
    expect(formatPrefsFrom({ dateFormat: 'DD/MM/YYYY', explicit: NOT_EXPLICIT })).toBeUndefined()
  })

  it('includes only the fields the server marks explicit', () => {
    expect(formatPrefsFrom({
      timezone: 'Europe/Oslo',
      dateFormat: 'DD/MM/YYYY',
      timeFormat: '24h',
      explicit: { ...NOT_EXPLICIT, timezone: true, dateFormat: true },
    })).toEqual({ timeZone: 'Europe/Oslo', dateFormat: 'DD/MM/YYYY' })
  })

  it('drops an explicit zone that is not a valid IANA zone, and an explicit auto', () => {
    expect(formatPrefsFrom({
      timezone: 'Not/AZone',
      dateFormat: 'auto',
      timeFormat: '12h',
      explicit: { ...NOT_EXPLICIT, timezone: true, dateFormat: true, timeFormat: true },
    })).toEqual({ timeFormat: '12h' })
  })
})

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json' },
    status,
  })
}

type Gateway = {
  gets: number
  patches: Array<Record<string, unknown>>
  server: Record<string, unknown>
  failRead: boolean
  failWrite: boolean
}

/** A stub for /api/v1/preferences: a GET returns the server copy, a PATCH updates it. */
function stubGateway(server: Record<string, unknown>): Gateway {
  const gateway: Gateway = { gets: 0, patches: [], server, failRead: false, failWrite: false }
  vi.stubGlobal('fetch', vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input) !== '/api/v1/preferences') return json({})
    if (init?.method === 'PATCH') {
      const body = JSON.parse(String(init.body)) as Record<string, unknown>
      gateway.patches.push(body)
      if (gateway.failWrite) return json({ error: { code: 'internal', message: 'boom' } }, 500)
      gateway.server = { ...gateway.server, ...body }
      return json({ message: 'Preferences updated successfully' })
    }
    gateway.gets += 1
    if (gateway.failRead) throw new TypeError('Failed to fetch')
    return json({ preferences: gateway.server })
  }))
  return gateway
}

function serverCopy(language: string): Record<string, unknown> {
  return { language, explicit: { ...NOT_EXPLICIT, language: true } }
}

/** A fresh copy of the store, so its locale cache is read from the storage this test has set up. */
async function loadStore() {
  vi.resetModules()
  return import('@/shared/preferences/preferences-store')
}

/** The bootstrap's dependencies. `emit` plays a locale change from the i18n provider. */
function bootDeps(current: Locale) {
  const applied: Locale[] = []
  let listener: ((event: LocaleEvent) => void) | undefined
  return {
    applied,
    emit: (event: LocaleEvent) => listener?.(event),
    deps: {
      getLocale: () => current,
      applyServerLocale: (locale: Locale) => {
        applied.push(locale)
      },
      subscribeLocaleChange: (next: (event: LocaleEvent) => void) => {
        listener = next
        return () => {
          listener = undefined
        }
      },
    },
  }
}

describe('startPreferencesBootstrap', () => {
  it('adopts a locale cached before this boot once: it wins over the server and is written through', async () => {
    window.localStorage.setItem('verevon.locale', 'en')
    const gateway = stubGateway(serverCopy('nb-NO'))
    const store = await loadStore()
    const { deps, applied } = bootDeps('en')
    stops.push(store.startPreferencesBootstrap(deps))

    await vi.waitFor(() => expect(gateway.patches).toEqual([{ language: 'en-US' }]))
    await vi.waitFor(() => expect(window.localStorage.getItem(LOCALE_SYNCED_KEY)).toBe('1'))
    expect(applied).toEqual([])
    expect(window.localStorage.getItem(LOCALE_DIRTY_KEY)).toBeNull()
  })

  it('applies an explicit, mapped server language when no locale was cached before this boot', async () => {
    const gateway = stubGateway(serverCopy('en-US'))
    const store = await loadStore()
    const { deps, applied } = bootDeps('no')
    stops.push(store.startPreferencesBootstrap(deps))

    await vi.waitFor(() => expect(applied).toEqual(['en']))
    expect(gateway.patches).toEqual([])
    expect(window.localStorage.getItem(LOCALE_SYNCED_KEY)).toBe('1')
  })

  it('leaves an unmapped server language such as fr-FR untouched', async () => {
    const gateway = stubGateway(serverCopy('fr-FR'))
    const store = await loadStore()
    const { deps, applied } = bootDeps('no')
    stops.push(store.startPreferencesBootstrap(deps))

    await vi.waitFor(() => {
      flush()
      expect(store.currentPreferences()?.language).toBe('fr-FR')
    })
    expect(applied).toEqual([])
    expect(gateway.patches).toEqual([])
    expect(window.localStorage.getItem(LOCALE_SYNCED_KEY)).toBeNull()
  })

  it('waits 500 ms after the last language toggle and writes only the latest locale', async () => {
    const gateway = stubGateway(serverCopy('nb-NO'))
    const store = await loadStore()
    const { deps, emit } = bootDeps('no')
    vi.useFakeTimers()
    stops.push(store.startPreferencesBootstrap(deps))
    await vi.advanceTimersByTimeAsync(0)

    emit({ locale: 'en', source: 'user' })
    await vi.advanceTimersByTimeAsync(300)
    emit({ locale: 'no', source: 'user' })
    await vi.advanceTimersByTimeAsync(LOCALE_WRITE_DEBOUNCE_MS - 1)
    expect(gateway.patches).toEqual([])
    await vi.advanceTimersByTimeAsync(1)
    expect(gateway.patches).toEqual([{ language: 'nb-NO' }])
  })

  it('leaves the write marked dirty when the PATCH fails, so the next boot retries it', async () => {
    const gateway = stubGateway(serverCopy('nb-NO'))
    gateway.failWrite = true
    const store = await loadStore()
    const { deps, emit } = bootDeps('no')
    vi.useFakeTimers()
    stops.push(store.startPreferencesBootstrap(deps))
    await vi.advanceTimersByTimeAsync(0)

    emit({ locale: 'en', source: 'user' })
    await vi.advanceTimersByTimeAsync(LOCALE_WRITE_DEBOUNCE_MS)
    expect(gateway.patches).toEqual([{ language: 'en-US' }])
    expect(window.localStorage.getItem(LOCALE_DIRTY_KEY)).not.toBeNull()
    expect(window.localStorage.getItem(LOCALE_SYNCED_KEY)).toBeNull()
  })

  it('retries a dirty local value at the next boot, ahead of the server copy', async () => {
    window.localStorage.setItem('verevon.locale', 'en')
    window.localStorage.setItem(LOCALE_DIRTY_KEY, '1')
    const gateway = stubGateway(serverCopy('nb-NO'))
    const store = await loadStore()
    const { deps, applied } = bootDeps('en')
    stops.push(store.startPreferencesBootstrap(deps))

    await vi.waitFor(() => expect(gateway.patches).toEqual([{ language: 'en-US' }]))
    await vi.waitFor(() => expect(window.localStorage.getItem(LOCALE_DIRTY_KEY)).toBeNull())
    expect(applied).toEqual([])
  })

  it('re-reads the preferences only after the tab was hidden for at least 60 s', async () => {
    const gateway = stubGateway(serverCopy('nb-NO'))
    const store = await loadStore()
    const { deps } = bootDeps('no')
    const visibility: { state: DocumentVisibilityState } = { state: 'visible' }
    Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility.state })
    vi.useFakeTimers()
    stops.push(store.startPreferencesBootstrap(deps))
    await vi.advanceTimersByTimeAsync(0)
    expect(gateway.gets).toBe(1)

    const hiddenFor = async (ms: number) => {
      visibility.state = 'hidden'
      document.dispatchEvent(new Event('visibilitychange'))
      await vi.advanceTimersByTimeAsync(ms)
      visibility.state = 'visible'
      document.dispatchEvent(new Event('visibilitychange'))
      await vi.advanceTimersByTimeAsync(0)
    }

    await hiddenFor(REREAD_AFTER_HIDDEN_MS - 1)
    expect(gateway.gets).toBe(1)
    await hiddenFor(REREAD_AFTER_HIDDEN_MS)
    expect(gateway.gets).toBe(2)
  })

  it('fails open when the read fails: nothing is applied or written and the cache stays empty', async () => {
    const gateway = stubGateway(serverCopy('en-US'))
    gateway.failRead = true
    const store = await loadStore()
    const { deps, applied } = bootDeps('no')
    stops.push(store.startPreferencesBootstrap(deps))

    await vi.waitFor(() => expect(gateway.gets).toBe(1))
    await new Promise((resolve) => setTimeout(resolve, 10))
    expect(applied).toEqual([])
    expect(gateway.patches).toEqual([])
    expect(store.currentPreferences()).toBeUndefined()
    expect(window.localStorage.getItem(LOCALE_SYNCED_KEY)).toBeNull()
  })
})
