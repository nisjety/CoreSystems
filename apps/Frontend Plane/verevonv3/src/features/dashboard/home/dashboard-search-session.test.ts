// @vitest-environment jsdom
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { searchSnapshotStorageKey } from '@/shared/session/home-local-content'
import { clearSession, markSessionOnboardingComplete, setSessionUser } from '@/shared/session/session-store'
import { currentSearchSnapshotKey, query, registerSearchRunner, resetSearchSession, runSearch, setQuery } from './dashboard-search-session'

const snapshotKey = searchSnapshotStorageKey('user-a', 'org-a')!
const otherUserKey = searchSnapshotStorageKey('user-b', 'org-a')!
const otherOrgKey = searchSnapshotStorageKey('user-a', 'org-b')!
const untouched = {
  'verevon.home-draft.v1:org-a:user-a': '{"text":"Keep this draft"}',
  'verevon.home-layout.v1:org-a:user-a': '{"dockMinimized":true}',
  'verevon.chat-organization.v1:org-a:user-a': '{"pinned":[]}',
  'verevon.home-web-history.v1:org-a:user-a': '[]',
}
const signIn = (id = 'user-a', org = 'org-a') => {
  setSessionUser({ id, email: `${id}@example.com`, name: id, emailVerified: true })
  flush()
  markSessionOnboardingComplete({ id: org, name: org, role: 'owner' })
  flush()
}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  setQuery('')
  flush()
  signIn()
})
afterEach(() => {
  clearSession()
  __resetRetentionForTests()
})

describe('currentSearchSnapshotKey', () => {
  it('is the signed-in user and workspace, read now, and null until both are known', () => {
    expect(currentSearchSnapshotKey()).toBe(snapshotKey)
    signIn('user-b')
    expect(currentSearchSnapshotKey()).toBe(otherUserKey)
    markSessionOnboardingComplete({ id: 'org-b', name: 'B', role: 'owner' }); flush()
    expect(currentSearchSnapshotKey()).toBe(searchSnapshotStorageKey('user-b', 'org-b'))
    clearSession(); flush()
    expect(currentSearchSnapshotKey()).toBeNull()
  })
})

describe('resetSearchSession', () => {
  it('empties the shared query and removes the saved snapshot from both storages', () => {
    setQuery('agent status'); flush()
    localStorage.setItem(snapshotKey, '{"query":"agent status"}')
    sessionStorage.setItem(snapshotKey, '{"query":"agent status"}')

    resetSearchSession(); flush()

    expect(query()).toBe('')
    expect(localStorage.getItem(snapshotKey)).toBeNull()
    expect(sessionStorage.getItem(snapshotKey)).toBeNull()
  })

  it("removes only the current user and workspace's snapshot", () => {
    localStorage.setItem(snapshotKey, '{"query":"mine"}')
    localStorage.setItem(otherUserKey, '{"query":"another user"}')
    localStorage.setItem(otherOrgKey, '{"query":"another workspace"}')

    resetSearchSession(); flush()

    expect(localStorage.getItem(snapshotKey)).toBeNull()
    expect(localStorage.getItem(otherUserKey)).not.toBeNull()
    expect(localStorage.getItem(otherOrgKey)).not.toBeNull()
  })

  it('leaves the chat draft and the other remembered Home state alone', () => {
    for (const [key, value] of Object.entries(untouched)) {
      localStorage.setItem(key, value)
      sessionStorage.setItem(key, value)
    }
    localStorage.setItem(snapshotKey, '{"query":"agent status"}')

    resetSearchSession(); flush()

    for (const [key, value] of Object.entries(untouched)) {
      expect(localStorage.getItem(key)).toBe(value)
      expect(sessionStorage.getItem(key)).toBe(value)
    }
    expect(localStorage.length).toBe(Object.keys(untouched).length)
  })

  it('is safe with nothing to reset, without an identity, and safe to repeat', () => {
    expect(() => { resetSearchSession(); resetSearchSession(); flush() }).not.toThrow()
    clearSession(); flush()
    setQuery('typed while signed out'); flush()
    expect(() => resetSearchSession()).not.toThrow()
    flush()
    expect(query()).toBe('')
  })
})

describe('the query kept in memory', () => {
  it('is emptied on sign-out, so the next person in the tab does not find it in the box', () => {
    setQuery('customer contract terms'); flush()
    clearSession(); flush()
    expect(query()).toBe('')
  })

  it('is emptied when a different user takes over the tab, but not for the same user', () => {
    setQuery('customer contract terms'); flush()
    setSessionUser({ id: 'user-a', email: 'user-a@example.com', name: 'user-a', emailVerified: true }); flush()
    expect(query()).toBe('customer contract terms')

    setSessionUser({ id: 'user-b', email: 'user-b@example.com', name: 'user-b', emailVerified: true }); flush()
    expect(query()).toBe('')
  })

  it('is emptied when the workspace turns out to be zero-data-retention', () => {
    setQuery('customer contract terms'); flush()
    applyServerRetention(true); flush()
    expect(query()).toBe('')
  })
})

describe('runSearch', () => {
  const withdraw: Array<() => void> = []
  afterEach(() => { for (const off of withdraw.splice(0)) off() })

  it('hands the trimmed text to the mounted panel and leaves the shared query to it', () => {
    const runner = vi.fn()
    withdraw.push(registerSearchRunner(runner))
    expect(runSearch('  agent status  ')).toBe(true)
    flush()
    expect(runner).toHaveBeenCalledTimes(1)
    expect(runner).toHaveBeenCalledWith('agent status', undefined)
    expect(query()).toBe('')
  })

  it('passes the options on to the panel', () => {
    const runner = vi.fn()
    withdraw.push(registerSearchRunner(runner))
    expect(runSearch('agent status', { focus: true })).toBe(true)
    expect(runner).toHaveBeenCalledWith('agent status', { focus: true })
  })

  it('ignores blank text', () => {
    const runner = vi.fn()
    withdraw.push(registerSearchRunner(runner))
    setQuery('kept'); flush()
    expect(runSearch('   ')).toBe(false)
    expect(runSearch('')).toBe(false)
    flush()
    expect(runner).not.toHaveBeenCalled()
    expect(query()).toBe('kept')
  })

  it('with no panel mounted only fills the query, and says it did not run', () => {
    expect(runSearch('agent status')).toBe(false)
    flush()
    expect(query()).toBe('agent status')
  })

  it('goes to the panel registered last, and only that panel can withdraw itself', () => {
    const first = vi.fn()
    const second = vi.fn()
    const offFirst = registerSearchRunner(first)
    const offSecond = registerSearchRunner(second)
    runSearch('one')
    expect(second).toHaveBeenCalledWith('one', undefined)
    expect(first).not.toHaveBeenCalled()
    // A remount registers the new panel before the old one is cleaned up; the late cleanup must not remove the new one.
    offFirst()
    runSearch('two')
    expect(second).toHaveBeenCalledWith('two', undefined)
    offSecond()
    expect(runSearch('three')).toBe(false)
    expect(second).toHaveBeenCalledTimes(2)
  })

  it('queues nothing for a panel that mounts later', () => {
    expect(runSearch('agent status')).toBe(false)
    const runner = vi.fn()
    withdraw.push(registerSearchRunner(runner))
    flush()
    expect(runner).not.toHaveBeenCalled()
  })
})
