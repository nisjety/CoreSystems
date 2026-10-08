// @vitest-environment jsdom
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  HOME_DRAFT_MAX_CHARS,
  capHomeDraft,
  clearHomeDrafts,
  homeDraftClearEpoch,
  homeDraftStorageKey,
  readHomeDraft,
  removeHomeDraft,
  writeHomeDraft,
} from './home-draft-storage'
import { clearSession, setSessionUser } from './session-store'

const keyA = homeDraftStorageKey('user-a', 'org-a')!
const keyB = homeDraftStorageKey('user-b', 'org-a')!
const user = (id: string) => ({ id, email: `${id}@example.com`, name: id, emailVerified: true })

beforeEach(() => {
  localStorage.clear(); sessionStorage.clear()
})
afterEach(() => vi.restoreAllMocks())

describe('home draft storage key', () => {
  it('is scoped by workspace and user, like the chat organizer', () => {
    expect(keyA).toBe('verevon.home-draft.v1:org-a:user-a')
    expect(homeDraftStorageKey('a:b/c', 'o rg')).toBe('verevon.home-draft.v1:o%20rg:a%3Ab%2Fc')
  })

  it('exists only when both the user and the workspace are known', () => {
    for (const [userId, orgId] of [[undefined, 'org-a'], ['user-a', undefined], [undefined, undefined], ['', 'org-a'], ['user-a', ''], ['  ', 'org-a'], ['user-a', ' ']] as const) {
      expect(homeDraftStorageKey(userId, orgId)).toBeNull()
    }
  })
})

describe('reading and writing the draft', () => {
  it('stores text in sessionStorage only', () => {
    const local = [vi.spyOn(window.localStorage, 'setItem'), vi.spyOn(window.localStorage, 'getItem')]
    expect(writeHomeDraft(keyA, 'Hei verden')).toBe('Hei verden')
    expect(JSON.parse(sessionStorage.getItem(keyA)!)).toEqual({ text: 'Hei verden' })
    expect(readHomeDraft(keyA)).toBe('Hei verden')
    for (const spy of local) expect(spy).not.toHaveBeenCalled()
  })

  it('never reads a copy left in localStorage', () => {
    localStorage.setItem(keyA, JSON.stringify({ text: 'old copy' }))
    expect(readHomeDraft(keyA)).toBe('')
  })

  it('keeps the text exactly as typed', () => {
    writeHomeDraft(keyA, '  linje 1\nlinje 2  ')
    expect(readHomeDraft(keyA)).toBe('  linje 1\nlinje 2  ')
  })

  it('does nothing without an identity', () => {
    const touched = [window.localStorage, window.sessionStorage].flatMap((storage) => (['getItem', 'setItem', 'removeItem'] as const).map((method) => vi.spyOn(storage, method)))
    expect(readHomeDraft(null)).toBe('')
    expect(writeHomeDraft(null, 'x')).toBeNull()
    removeHomeDraft(null)
    for (const spy of touched) expect(spy).not.toHaveBeenCalled()
  })

  it('removes the entry for blank text instead of storing it', () => {
    writeHomeDraft(keyA, 'noe')
    expect(writeHomeDraft(keyA, '')).toBe('')
    expect(sessionStorage.getItem(keyA)).toBeNull()
    writeHomeDraft(keyA, 'noe')
    expect(writeHomeDraft(keyA, ' \n\t ')).toBe('')
    expect(sessionStorage.getItem(keyA)).toBeNull()
  })

  it('caps the length, without splitting a surrogate pair', () => {
    const long = 'a'.repeat(HOME_DRAFT_MAX_CHARS + 500)
    expect(writeHomeDraft(keyA, long)).toHaveLength(HOME_DRAFT_MAX_CHARS)
    expect(readHomeDraft(keyA)).toHaveLength(HOME_DRAFT_MAX_CHARS)
    const pair = `${'a'.repeat(HOME_DRAFT_MAX_CHARS - 1)}\u{1F600}b`
    const capped = capHomeDraft(pair)
    expect(capped).toBe('a'.repeat(HOME_DRAFT_MAX_CHARS - 1))
    expect(capHomeDraft('kort')).toBe('kort')
  })

  it('caps what it reads, in case the value was edited by hand', () => {
    sessionStorage.setItem(keyA, JSON.stringify({ text: 'a'.repeat(HOME_DRAFT_MAX_CHARS * 2) }))
    expect(readHomeDraft(keyA)).toHaveLength(HOME_DRAFT_MAX_CHARS)
  })

  it('falls back to an empty draft for anything that is not a stored draft', () => {
    for (const raw of ['not json', '{', 'null', '[]', '"text"', '42', '{}', '{"text":5}', '{"text":null}', '{"body":"x"}']) {
      sessionStorage.setItem(keyA, raw)
      expect(readHomeDraft(keyA)).toBe('')
    }
  })

  it('reports a write that storage refused', () => {
    vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => { throw new DOMException('full', 'QuotaExceededError') })
    expect(writeHomeDraft(keyA, 'noe')).toBeNull()
  })

  it('survives a storage that cannot be read', () => {
    vi.spyOn(window.sessionStorage, 'getItem').mockImplementation(() => { throw new DOMException('blocked', 'SecurityError') })
    expect(readHomeDraft(keyA)).toBe('')
  })
})

describe('clearing the drafts', () => {
  it('removes every account draft and nothing else', () => {
    writeHomeDraft(keyA, 'a')
    writeHomeDraft(keyB, 'b')
    sessionStorage.setItem('verevon.chat.pendingLaunch', 'keep')
    localStorage.setItem('verevon.home-layout.v1:org-a:user-a', 'keep')
    const before = homeDraftClearEpoch()

    clearHomeDrafts()

    expect(sessionStorage.getItem(keyA)).toBeNull()
    expect(sessionStorage.getItem(keyB)).toBeNull()
    expect(sessionStorage.getItem('verevon.chat.pendingLaunch')).toBe('keep')
    expect(localStorage.getItem('verevon.home-layout.v1:org-a:user-a')).toBe('keep')
    expect(homeDraftClearEpoch()).toBe(before + 1)
  })

  it('happens when the session is cleared (sign-out)', () => {
    writeHomeDraft(keyA, 'a')
    writeHomeDraft(keyB, 'b')
    clearSession()
    expect(sessionStorage.getItem(keyA)).toBeNull()
    expect(sessionStorage.getItem(keyB)).toBeNull()
  })

  it('happens when a different user takes over the tab, but not for the same user', () => {
    setSessionUser(user('user-a'))
    flush()
    writeHomeDraft(keyA, 'a')
    setSessionUser(user('user-a'))
    flush()
    expect(readHomeDraft(keyA)).toBe('a')

    setSessionUser(user('user-b'))
    flush()
    expect(sessionStorage.getItem(keyA)).toBeNull()
    clearSession()
  })
})
