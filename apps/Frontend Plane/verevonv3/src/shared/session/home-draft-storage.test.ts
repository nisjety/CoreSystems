// @vitest-environment jsdom
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  CHAT_DRAFT_MAX_THREADS,
  HOME_DRAFT_MAX_CHARS,
  capHomeDraft,
  chatDraftClearEpoch,
  chatDraftStorageKey,
  clearChatDrafts,
  clearHomeDrafts,
  homeDraftClearEpoch,
  homeDraftStorageKey,
  readHomeDraft,
  removeChatDraft,
  removeHomeDraft,
  writeChatDraft,
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

describe('chat thread drafts', () => {
  const chatKey = (thread: string) => chatDraftStorageKey('user-a', 'org-a', thread)!
  // The keys of the stored drafts only: the index of the drafts is not one of them.
  const chatDraftKeys = () => {
    const keys: string[] = []
    for (let index = 0; index < sessionStorage.length; index += 1) {
      const key = sessionStorage.key(index)
      if (key?.startsWith('verevon.chat-draft.v1:') && !Array.isArray(JSON.parse(sessionStorage.getItem(key)!))) keys.push(key)
    }
    return keys
  }

  it('is keyed by workspace, user and thread, and exists only when all three are known', () => {
    expect(chatKey('thread-1')).toBe('verevon.chat-draft.v1:org-a:user-a:thread-1')
    expect(chatDraftStorageKey('user-a', 'org-a', 'new')).toBe('verevon.chat-draft.v1:org-a:user-a:new')
    expect(chatDraftStorageKey(undefined, 'org-a', 'thread-1')).toBeNull()
    expect(chatDraftStorageKey('user-a', ' ', 'thread-1')).toBeNull()
    expect(chatDraftStorageKey('user-a', 'org-a', '  ')).toBeNull()
  })

  it('never shares a key with the Home draft, whose storage it leaves as it was', () => {
    expect(chatKey('new')).not.toBe(keyA)
    writeChatDraft(chatKey('new'), 'chat')
    writeHomeDraft(keyA, 'home')
    expect(readHomeDraft(keyA)).toBe('home')
    expect(readHomeDraft(chatKey('new'))).toBe('chat')
  })

  it('stores the text in sessionStorage, and removes the entry for blank text', () => {
    expect(writeChatDraft(chatKey('thread-1'), 'Hei')).toBe('Hei')
    expect(JSON.parse(sessionStorage.getItem(chatKey('thread-1'))!)).toEqual({ text: 'Hei' })
    expect(readHomeDraft(chatKey('thread-1'))).toBe('Hei')
    expect(writeChatDraft(chatKey('thread-1'), ' \n ')).toBe('')
    expect(sessionStorage.getItem(chatKey('thread-1'))).toBeNull()
    expect(chatDraftKeys()).toEqual([])
  })

  it('caps the length as the Home draft does', () => {
    expect(writeChatDraft(chatKey('thread-1'), 'a'.repeat(HOME_DRAFT_MAX_CHARS + 5))).toHaveLength(HOME_DRAFT_MAX_CHARS)
  })

  it('keeps at most thirty thread drafts, and removes the least recently written one first', () => {
    for (let index = 0; index < CHAT_DRAFT_MAX_THREADS; index += 1) writeChatDraft(chatKey(`thread-${index}`), `draft ${index}`)
    expect(chatDraftKeys()).toHaveLength(CHAT_DRAFT_MAX_THREADS)

    // Writing thread-0 again makes it the most recent, so thread-1 is the one that goes.
    writeChatDraft(chatKey('thread-0'), 'draft 0 again')
    writeChatDraft(chatKey('thread-new-one'), 'the newest')

    expect(chatDraftKeys()).toHaveLength(CHAT_DRAFT_MAX_THREADS)
    expect(readHomeDraft(chatKey('thread-1'))).toBe('')
    expect(readHomeDraft(chatKey('thread-0'))).toBe('draft 0 again')
    expect(readHomeDraft(chatKey('thread-new-one'))).toBe('the newest')
  })

  it('forgets a removed draft, so it does not count toward the thirty', () => {
    for (let index = 0; index < CHAT_DRAFT_MAX_THREADS; index += 1) writeChatDraft(chatKey(`thread-${index}`), 'x')
    removeChatDraft(chatKey('thread-0'))
    writeChatDraft(chatKey('thread-extra'), 'y')
    expect(chatDraftKeys()).toHaveLength(CHAT_DRAFT_MAX_THREADS)
    expect(readHomeDraft(chatKey('thread-1'))).toBe('x')
  })

  it('is removed by clearing the chat drafts, and clearing the Home drafts leaves it alone', () => {
    writeHomeDraft(keyA, 'home')
    writeChatDraft(chatKey('thread-1'), 'chat')
    const before = chatDraftClearEpoch()

    clearHomeDrafts()
    expect(readHomeDraft(chatKey('thread-1'))).toBe('chat')

    clearChatDrafts()
    expect(chatDraftKeys()).toEqual([])
    expect(sessionStorage.length).toBe(0)
    expect(chatDraftClearEpoch()).toBe(before + 1)
  })
})
