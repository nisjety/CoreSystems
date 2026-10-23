// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { __resetRetentionForTests } from '@/features/chat/lib/chat-retention'
import { omniboxHistoryStorageKey } from '@/shared/session/home-local-content'
import { addOmniboxVisit } from './omnibox-history'
import { createOmniboxSuggestions, matchingHistory, type OmniboxRow } from './omnibox-suggestions'

const keyA = omniboxHistoryStorageKey('user-a', 'org-a')!

function harness(overrides: Partial<Parameters<typeof createOmniboxSuggestions>[0]> = {}) {
  let latest: OmniboxRow[] = []
  const fetchSuggestions = vi.fn(async () => [{ text: 'norwegian salmon exports' }, { text: '  ' }])
  const controller = createOmniboxSuggestions({
    debounceMs: 100,
    historyKey: () => keyA,
    searchEnabled: true,
    askEnabled: true,
    zdr: () => false,
    fetchSuggestions,
    onRows: (rows) => {
      latest = rows
    },
    ...overrides,
  })
  return { controller, fetchSuggestions, rows: () => latest }
}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  __resetRetentionForTests()
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('createOmniboxSuggestions', () => {
  it('offers nothing below two characters', () => {
    const { controller, rows, fetchSuggestions } = harness()
    controller.update('a')
    vi.advanceTimersByTime(500)
    expect(rows()).toEqual([])
    expect(fetchSuggestions).not.toHaveBeenCalled()
  })

  it('shows the local and explicit rows at once, then the suggestions after the debounce', async () => {
    addOmniboxVisit(keyA, { url: 'https://www.vg.no/nyheter', title: 'VG nyheter' })
    const { controller, rows, fetchSuggestions } = harness()

    controller.update('vg')
    expect(rows().map((row) => row.kind)).toEqual(['history', 'search', 'ask'])
    expect(fetchSuggestions).not.toHaveBeenCalled()

    vi.advanceTimersByTime(100)
    await vi.runAllTimersAsync()
    expect(fetchSuggestions).toHaveBeenCalledTimes(1)
    expect(rows().map((row) => row.kind)).toEqual(['history', 'suggestion', 'search', 'ask'])
    expect(rows()[1]).toEqual({ kind: 'suggestion', id: 'suggestion-0', text: 'norwegian salmon exports' })
  })

  it('aborts the pending request when the text changes', () => {
    const seen: AbortSignal[] = []
    const fetchSuggestions = vi.fn((_query: string, signal: AbortSignal) => {
      seen.push(signal)
      return new Promise<[]>(() => undefined)
    })
    const { controller } = harness({ fetchSuggestions })

    controller.update('norw')
    vi.advanceTimersByTime(100)
    controller.update('norway')
    expect(seen[0]?.aborted).toBe(true)
  })

  it('makes no request and reads no history under zero data retention', async () => {
    addOmniboxVisit(keyA, { url: 'https://www.vg.no/nyheter', title: 'VG nyheter' })
    const getItem = vi.spyOn(Storage.prototype, 'getItem')
    const { controller, rows, fetchSuggestions } = harness({ zdr: () => true })
    getItem.mockClear()
    const fetchSpy = vi.fn()
    vi.stubGlobal('fetch', fetchSpy)

    controller.update('norwegian salmon')
    await vi.runAllTimersAsync()

    expect(fetchSuggestions).not.toHaveBeenCalled()
    expect(fetchSpy).not.toHaveBeenCalled()
    vi.unstubAllGlobals()
    expect(getItem.mock.calls.some(([key]) => key === keyA)).toBe(false)
    expect(rows().map((row) => row.kind)).toEqual(['search', 'ask'])
  })

  it('leaves out the search row when search is off, and asks for no suggestions', async () => {
    const { controller, rows, fetchSuggestions } = harness({ searchEnabled: false })
    controller.update('norwegian salmon')
    await vi.runAllTimersAsync()
    expect(fetchSuggestions).not.toHaveBeenCalled()
    expect(rows().map((row) => row.kind)).toEqual(['ask'])
  })

  it('leaves out the ask row when no hand-off to chat is wired', () => {
    const { controller, rows } = harness({ askEnabled: false })
    controller.update('norwegian salmon')
    expect(rows().map((row) => row.kind)).toEqual(['search'])
  })

  it('stops the pending timer on dispose', async () => {
    const { controller, fetchSuggestions } = harness()
    controller.update('norway')
    controller.dispose()
    await vi.runAllTimersAsync()
    expect(fetchSuggestions).not.toHaveBeenCalled()
  })

  it('keeps the local rows when the suggestion request fails', async () => {
    addOmniboxVisit(keyA, { url: 'https://www.vg.no/nyheter', title: 'VG nyheter' })
    const fetchSuggestions = vi.fn(async () => {
      throw new Error('offline')
    })
    const { controller, rows } = harness({ fetchSuggestions })
    controller.update('vg')
    await vi.runAllTimersAsync()
    expect(rows().map((row) => row.kind)).toEqual(['history', 'search', 'ask'])
  })
})

describe('matchingHistory', () => {
  it('matches the address or the title, case-insensitively, up to the row limit', () => {
    const entries = [
      { url: 'https://www.vg.no/', title: 'VG', lastVisitedAt: '2026-10-23T10:00:00.000Z', visits: 1 },
      { url: 'https://example.com/', title: 'Example Domain', lastVisitedAt: '2026-10-23T09:00:00.000Z', visits: 1 },
    ]
    expect(matchingHistory(entries, 'VG').map((entry) => entry.url)).toEqual(['https://www.vg.no/'])
    expect(matchingHistory(entries, 'domain').map((entry) => entry.url)).toEqual(['https://example.com/'])
    expect(matchingHistory(entries, 'a')).toEqual([])
  })
})
