// @vitest-environment jsdom
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { homeDraftStorageKey, readHomeDraft, writeHomeDraft } from './home-draft-storage'
import {
  clearHomeLocalContent,
  clearSearchSnapshots,
  clearWebHistories,
  onSearchSnapshotsCleared,
  searchSnapshotStorageKey,
  webHistoryStorageKey,
} from './home-local-content'
import { clearSession, setSessionUser } from './session-store'

const snapshotA = searchSnapshotStorageKey('user-a', 'org-a')!
const snapshotB = searchSnapshotStorageKey('user-b', 'org-a')!
const historyA = webHistoryStorageKey('user-a', 'org-a')!
const historyB = webHistoryStorageKey('user-b', 'org-a')!
const legacySnapshotKey = 'verevon.dashboard.search.session'
const user = (id: string) => ({ id, email: `${id}@example.com`, name: id, emailVerified: true })
const put = (key: string) => { localStorage.setItem(key, '{"query":"x"}'); sessionStorage.setItem(key, '{"query":"x"}') }
const present = (key: string) => localStorage.getItem(key) !== null || sessionStorage.getItem(key) !== null

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
})
afterEach(() => vi.restoreAllMocks())

describe('keys', () => {
  it('are scoped by workspace and user, like the chat organizer', () => {
    expect(snapshotA).toBe('verevon.dashboard.search.session:org-a:user-a')
    expect(historyA).toBe('verevon.home-web-history.v1:org-a:user-a')
    expect(searchSnapshotStorageKey('a:b/c', 'o rg')).toBe('verevon.dashboard.search.session:o%20rg:a%3Ab%2Fc')
    expect(webHistoryStorageKey('a:b/c', 'o rg')).toBe('verevon.home-web-history.v1:o%20rg:a%3Ab%2Fc')
  })

  it('exist only when both the user and the workspace are known', () => {
    for (const [userId, orgId] of [[undefined, 'org-a'], ['user-a', undefined], ['', 'org-a'], ['user-a', ''], ['  ', 'org-a'], ['user-a', ' ']] as const) {
      expect(searchSnapshotStorageKey(userId, orgId)).toBeNull()
      expect(webHistoryStorageKey(userId, orgId)).toBeNull()
    }
  })
})

describe('clearing the saved Nett search', () => {
  it('removes every account\'s snapshot from both storages, and the unscoped key older builds wrote', () => {
    for (const key of [snapshotA, snapshotB, legacySnapshotKey]) put(key)
    put(historyA)
    localStorage.setItem('verevon.home-layout.v1:org-a:user-a', 'keep')

    clearSearchSnapshots()

    for (const key of [snapshotA, snapshotB, legacySnapshotKey]) expect(present(key)).toBe(false)
    expect(present(historyA)).toBe(true)
    expect(localStorage.getItem('verevon.home-layout.v1:org-a:user-a')).toBe('keep')
  })

  it('tells the listeners after the storage is empty, and lets one withdraw', () => {
    put(snapshotA)
    const seen: boolean[] = []
    const off = onSearchSnapshotsCleared(() => seen.push(present(snapshotA)))
    const withdrawn = vi.fn()
    onSearchSnapshotsCleared(withdrawn)()

    clearSearchSnapshots()
    off()
    clearSearchSnapshots()

    expect(seen).toEqual([false])
    expect(withdrawn).not.toHaveBeenCalled()
  })

  it('is not stopped by a listener that throws', () => {
    put(snapshotA)
    const after = vi.fn()
    const offThrowing = onSearchSnapshotsCleared(() => { throw new Error('listener failed') })
    const offAfter = onSearchSnapshotsCleared(after)

    expect(() => clearSearchSnapshots()).not.toThrow()

    expect(after).toHaveBeenCalledTimes(1)
    offThrowing(); offAfter()
  })

  it('happens when the session is cleared (sign-out)', () => {
    put(snapshotA); put(snapshotB); put(legacySnapshotKey)
    clearSession()
    for (const key of [snapshotA, snapshotB, legacySnapshotKey]) expect(present(key)).toBe(false)
  })

  it('happens when a different user takes over the tab, but not for the same user', () => {
    setSessionUser(user('user-a'))
    flush()
    put(snapshotA)
    setSessionUser(user('user-a'))
    flush()
    expect(present(snapshotA)).toBe(true)

    setSessionUser(user('user-b'))
    flush()
    expect(present(snapshotA)).toBe(false)
    clearSession()
  })

  it('leaves the Nett history in place at sign-out: its identity key is the isolation', () => {
    put(historyA)
    clearSession()
    expect(present(historyA)).toBe(true)
  })
})

describe('clearing the Nett history', () => {
  it('removes every account\'s history from both storages and nothing else', () => {
    put(historyA); put(historyB); put(snapshotA)
    clearWebHistories()
    expect(present(historyA)).toBe(false)
    expect(present(historyB)).toBe(false)
    expect(present(snapshotA)).toBe(true)
  })
})

describe('clearing everything Home keeps (zero data retention)', () => {
  it('removes drafts, saved searches and Nett histories', () => {
    const draft = homeDraftStorageKey('user-a', 'org-a')
    writeHomeDraft(draft, 'confidential')
    put(snapshotA); put(historyA)
    localStorage.setItem('verevon.home-layout.v1:org-a:user-a', 'keep')

    clearHomeLocalContent()

    expect(readHomeDraft(draft)).toBe('')
    expect(present(snapshotA)).toBe(false)
    expect(present(historyA)).toBe(false)
    expect(localStorage.getItem('verevon.home-layout.v1:org-a:user-a')).toBe('keep')
  })
})
