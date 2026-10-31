// @vitest-environment jsdom
import { createRoot, flush } from 'solid-js'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const identity = vi.hoisted(() => ({ user: { id: 'user-a' } as { id: string } | null, activeOrg: { id: 'org-a' } as { id: string } | null }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))

import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { omniboxHistoryStorageKey } from '@/shared/session/home-local-content'
import { clearHomeLocalContent } from '@/shared/session/home-local-content'
import {
  OMNIBOX_HISTORY_MAX_ENTRIES,
  addOmniboxVisit,
  clearOmniboxHistory,
  deleteOmniboxEntry,
  readOmniboxHistory,
  recordOmniboxVisit,
  removeOmniboxEntry,
  scrubOmniboxUrl,
  useOmniboxHistory,
} from './omnibox-history'

const keyA = omniboxHistoryStorageKey('user-a', 'org-a')!
const keyB = omniboxHistoryStorageKey('user-b', 'org-a')!
const at = (minute: number) => new Date(Date.UTC(2026, 9, 23, 10, minute))

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  __resetRetentionForTests()
  identity.user = { id: 'user-a' }
  identity.activeOrg = { id: 'org-a' }
})

describe('scrubOmniboxUrl', () => {
  it('keeps a short query and drops a credential-looking one', () => {
    expect(scrubOmniboxUrl('https://example.com/search?q=a')).toBe('https://example.com/search?q=a')
    expect(scrubOmniboxUrl('https://example.com/?token=x')).toBe('https://example.com/')
    expect(scrubOmniboxUrl('https://example.com/?session_id=1&page=2')).toBe('https://example.com/')
  })

  it('drops a query longer than 256 characters, removes credentials and the fragment', () => {
    const long = `https://example.com/?q=${'a'.repeat(300)}`
    expect(scrubOmniboxUrl(long)).toBe('https://example.com/')
    expect(scrubOmniboxUrl('https://user:pw@example.com/path#frag')).toBe('https://example.com/path')
  })

  it('refuses anything that is not an absolute http(s) url', () => {
    expect(scrubOmniboxUrl('javascript:alert(1)')).toBeNull()
    expect(scrubOmniboxUrl('about:blank')).toBeNull()
    expect(scrubOmniboxUrl(42)).toBeNull()
  })
})

describe('keys', () => {
  it('are scoped by workspace and user, and exist only when both are known', () => {
    expect(keyA).toBe('verevon.browser-omnibox.v1:org-a:user-a')
    expect(omniboxHistoryStorageKey('user-a', undefined)).toBeNull()
    expect(omniboxHistoryStorageKey(undefined, 'org-a')).toBeNull()
  })
})

describe('addOmniboxVisit and readOmniboxHistory', () => {
  it('stores visits newest first, counts repeats and keeps one row per URL', () => {
    addOmniboxVisit(keyA, { url: 'https://www.vg.no/', title: 'VG' }, at(1))
    addOmniboxVisit(keyA, { url: 'https://example.com/', title: 'Example' }, at(2))
    const again = addOmniboxVisit(keyA, { url: 'https://www.vg.no/', title: 'VG' }, at(3))

    expect(again.map((entry) => entry.url)).toEqual(['https://www.vg.no/', 'https://example.com/'])
    expect(again[0]).toMatchObject({ visits: 2, lastVisitedAt: at(3).toISOString() })
    expect(readOmniboxHistory(keyA)).toEqual(again)
  })

  it('caps the record and never stores page text or form values', () => {
    for (let index = 0; index < OMNIBOX_HISTORY_MAX_ENTRIES + 5; index += 1) {
      addOmniboxVisit(keyA, { url: `https://site${index}.example/` }, new Date(Date.UTC(2026, 0, 1, 0, index)))
    }
    const stored = readOmniboxHistory(keyA)
    expect(stored).toHaveLength(OMNIBOX_HISTORY_MAX_ENTRIES)
    expect(Object.keys(stored[0] ?? {}).sort()).toEqual(['lastVisitedAt', 'title', 'url', 'visits'])
  })

  it('reads a corrupt record as empty and skips entries that fail the scrub', () => {
    localStorage.setItem(keyA, '{not json')
    expect(readOmniboxHistory(keyA)).toEqual([])
    localStorage.setItem(
      keyA,
      JSON.stringify([
        { url: 'javascript:alert(1)', lastVisitedAt: at(1).toISOString() },
        { url: 'https://ok.example/', lastVisitedAt: at(2).toISOString() },
      ]),
    )
    expect(readOmniboxHistory(keyA).map((entry) => entry.url)).toEqual(['https://ok.example/'])
  })

  it('is isolated per user and workspace', () => {
    addOmniboxVisit(keyA, { url: 'https://a.example/' })
    expect(readOmniboxHistory(keyB)).toEqual([])
  })
})

describe('removal and purge', () => {
  it('removes one URL and leaves the rest', () => {
    addOmniboxVisit(keyA, { url: 'https://a.example/' }, at(1))
    addOmniboxVisit(keyA, { url: 'https://b.example/' }, at(2))
    expect(removeOmniboxEntry(keyA, 'https://a.example/').map((entry) => entry.url)).toEqual(['https://b.example/'])
  })

  it('clearHomeLocalContent removes every account\'s omnibox history', () => {
    addOmniboxVisit(keyA, { url: 'https://a.example/' })
    addOmniboxVisit(keyB, { url: 'https://b.example/' })
    clearHomeLocalContent()
    expect(readOmniboxHistory(keyA)).toEqual([])
    expect(readOmniboxHistory(keyB)).toEqual([])
  })
})

describe('recording from the live session', () => {
  it('records a visit for the current identity when the workspace allows local copies', () => {
    recordOmniboxVisit({ url: 'https://www.vg.no/', title: 'VG' }, { zdr: false })
    expect(readOmniboxHistory(keyA).map((entry) => entry.url)).toEqual(['https://www.vg.no/'])
  })

  it('writes nothing for a session that is zero-data-retention', () => {
    recordOmniboxVisit({ url: 'https://www.vg.no/' }, { zdr: true })
    expect(localStorage.length).toBe(0)
  })

  it('writes nothing once the workspace retention verdict is zero-data-retention', () => {
    applyServerRetention(true)
    recordOmniboxVisit({ url: 'https://www.vg.no/' }, { zdr: false })
    expect(localStorage.length).toBe(0)
  })

  it('writes nothing before the user and workspace are known', () => {
    identity.activeOrg = null
    recordOmniboxVisit({ url: 'https://www.vg.no/' }, { zdr: false })
    expect(localStorage.length).toBe(0)
  })

  it('forgets one entry or everything for the current identity only', () => {
    recordOmniboxVisit({ url: 'https://a.example/' }, { zdr: false })
    identity.user = { id: 'user-b' }
    recordOmniboxVisit({ url: 'https://b.example/' }, { zdr: false })
    identity.user = { id: 'user-a' }
    deleteOmniboxEntry('https://a.example/')
    expect(readOmniboxHistory(keyA)).toEqual([])
    recordOmniboxVisit({ url: 'https://a.example/' }, { zdr: false })
    clearOmniboxHistory()
    expect(readOmniboxHistory(keyA)).toEqual([])
    expect(readOmniboxHistory(keyB).map((entry) => entry.url)).toEqual(['https://b.example/'])
  })

  it('useOmniboxHistory follows a recorded visit', () => {
    createRoot((dispose) => {
      const history = useOmniboxHistory()
      expect(history()).toEqual([])
      recordOmniboxVisit({ url: 'https://www.vg.no/' }, { zdr: false })
      flush()
      expect(history().map((entry) => entry.url)).toEqual(['https://www.vg.no/'])
      dispose()
    })
  })
})
