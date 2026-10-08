import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  __resetRetentionForTests,
  applyServerRetention,
  forgetThreadLocally,
  isLocalRetentionAllowed,
  reconcileLocalThreads,
  subscribeLocalRetention,
} from '@/features/chat/lib/chat-retention'
import {
  clearActiveChatThreadId,
  clearChatThreadHistory,
  readChatThreadHistory,
  readChatThreadTranscript,
  setActiveChatThreadId,
  upsertChatThreadHistory,
  upsertChatThreadTranscript,
} from '@/features/chat/lib/chat-thread-history'
import { readClientValue, writeClientJson } from '@/shared/session/client-storage'
import { homeDraftStorageKey, readHomeDraft, writeHomeDraft } from '@/shared/session/home-draft-storage'
import { searchSnapshotStorageKey, webHistoryStorageKey } from '@/shared/session/home-local-content'

function createStorageMock(): Storage {
  const entries = new Map<string, string>()

  return {
    clear: vi.fn(() => entries.clear()),
    getItem: vi.fn((key: string) => entries.get(key) ?? null),
    key: vi.fn((index: number) => Array.from(entries.keys())[index] ?? null),
    get length() {
      return entries.size
    },
    removeItem: vi.fn((key: string) => entries.delete(key)),
    setItem: vi.fn((key: string, value: string) => entries.set(key, value)),
  }
}

/** Store one thread with both an index entry and a full transcript. */
function storeThread(threadId: string, updatedAt: string): void {
  upsertChatThreadHistory({
    threadId,
    title: 'Kvartalstall',
    preview: 'Omsetningen endte på 4,2 mrd',
    updatedAt,
  })
  upsertChatThreadTranscript({
    threadId,
    updatedAt,
    turns: [{ id: 'user_1', role: 'user', content: 'hemmelig', createdAt: updatedAt }],
  })
}

const LONG_AGO = '2020-01-01T00:00:00.000Z'

describe('chat retention', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    Object.defineProperty(window, 'sessionStorage', {
      configurable: true,
      value: createStorageMock(),
    })
    window.localStorage.clear()
    window.sessionStorage.clear()
    __resetRetentionForTests()
  })

  afterEach(() => {
    clearActiveChatThreadId()
    clearChatThreadHistory()
    __resetRetentionForTests()
  })

  it('allows local retention until the server says otherwise', () => {
    // A denied default would wipe every user's sidebar on first paint, before
    // any listing has returned.
    expect(isLocalRetentionAllowed()).toBe(true)
  })

  /**
   * The defect this exists to close: the SPA kept a full transcript copy on the
   * device gated only by an in-memory Set, and it is the one copy no
   * server-side erasure can reach. When the org turns ZDR on, it has to go.
   */
  it('purges every local copy the moment the server reports a ZDR posture', () => {
    storeThread('thread-1', LONG_AGO)
    expect(readChatThreadTranscript('thread-1')).not.toBeNull()

    applyServerRetention(true)

    expect(isLocalRetentionAllowed()).toBe(false)
    expect(readChatThreadHistory()).toEqual([])
    expect(readChatThreadTranscript('thread-1')).toBeNull()
  })

  it('restores local retention when the posture is turned back off', () => {
    applyServerRetention(true)
    expect(isLocalRetentionAllowed()).toBe(false)
    applyServerRetention(false)
    expect(isLocalRetentionAllowed()).toBe(true)
  })

  /**
   * An older gateway that does not state a posture must not be read as either
   * permission or denial — inventing either from a missing field is how a
   * policy silently flips.
   */
  it('treats an absent posture as no change', () => {
    storeThread('thread-1', LONG_AGO)
    applyServerRetention(undefined)
    expect(isLocalRetentionAllowed()).toBe(true)
    expect(readChatThreadHistory()).toHaveLength(1)

    applyServerRetention(true)
    applyServerRetention(undefined)
    expect(isLocalRetentionAllowed()).toBe(false)
  })

  it('forgets one thread when a save came back unretained', () => {
    storeThread('thread-1', LONG_AGO)
    storeThread('thread-2', LONG_AGO)

    forgetThreadLocally('thread-1')

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['thread-2'])
    expect(readChatThreadTranscript('thread-1')).toBeNull()
    expect(readChatThreadTranscript('thread-2')).not.toBeNull()
  })

  /**
   * How a GDPR erasure finally reaches the device: once the gateway and
   * session-core have both purged a conversation it stops appearing in the
   * listing, and the local copy goes with it.
   */
  it('drops local threads the server no longer lists', () => {
    storeThread('erased', LONG_AGO)
    storeThread('kept', LONG_AGO)

    reconcileLocalThreads(['kept'])

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['kept'])
    expect(readChatThreadTranscript('erased')).toBeNull()
  })

  /**
   * The race this guard exists for: the SPA debounces its snapshot save by
   * 500ms, so a chat being typed into right now may legitimately not be in the
   * server listing yet. Reconciling it away would delete the live conversation.
   */
  it('never reconciles away a freshly-updated thread', () => {
    storeThread('brand-new', new Date().toISOString())

    reconcileLocalThreads([])

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['brand-new'])
  })

  it('never reconciles away the active thread', () => {
    storeThread('active-but-old', LONG_AGO)
    setActiveChatThreadId('active-but-old')

    reconcileLocalThreads([])

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['active-but-old'])
  })

  /**
   * An unparsable timestamp must read as recent, not ancient. Guessing "old"
   * would delete it.
   */
  it('keeps a thread whose timestamp cannot be parsed', () => {
    upsertChatThreadHistory({
      threadId: 'no-timestamp',
      title: 'T',
      preview: 'p',
      updatedAt: 'not-a-date',
    })

    reconcileLocalThreads([])

    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['no-timestamp'])
  })

  describe('Home local copies', () => {
    const draftKey = homeDraftStorageKey('user-a', 'org-a')
    const snapshotKey = searchSnapshotStorageKey('user-a', 'org-a')!
    const historyKey = webHistoryStorageKey('user-a', 'org-a')!
    // Subscribers outlive a test unless they are withdrawn.
    const withdrawals: Array<() => void> = []
    const subscribe = (listener: (allowed: boolean) => void) => {
      const withdraw = subscribeLocalRetention(listener)
      withdrawals.push(withdraw)
      return withdraw
    }
    afterEach(() => {
      for (const withdraw of withdrawals.splice(0)) withdraw()
    })
    const storeHomeContent = () => {
      writeHomeDraft(draftKey, 'Kundens kontrakt')
      writeClientJson(snapshotKey, { query: 'kontraktsvilkår', savedAt: Date.now() })
      writeClientJson(historyKey, [{ kind: 'query', text: 'kontraktsvilkår', at: '2026-09-29T10:00:00.000Z' }])
    }

    /**
     * Home keeps its own copies of what a person typed or searched for. They are
     * on the same device, beyond the same erasure path, so the same verdict
     * purges them.
     */
    it('purges the Home draft, the saved Nett search and the Nett history on a ZDR posture', () => {
      storeHomeContent()

      applyServerRetention(true)

      expect(readHomeDraft(draftKey)).toBe('')
      expect(readClientValue(snapshotKey)).toBeNull()
      expect(readClientValue(historyKey)).toBeNull()
      expect(window.localStorage.length + window.sessionStorage.length).toBe(0)
    })

    it('leaves them alone while the posture is allowed, unchanged or unstated', () => {
      storeHomeContent()

      applyServerRetention(false)
      applyServerRetention(undefined)

      expect(readHomeDraft(draftKey)).toBe('Kundens kontrakt')
      expect(readClientValue(snapshotKey)).not.toBeNull()
      expect(readClientValue(historyKey)).not.toBeNull()
    })

    it('tells subscribers each change of posture once, after the purge, and not a repeat', () => {
      const heard: Array<{ allowed: boolean; draft: string }> = []
      subscribe((allowed) => heard.push({ allowed, draft: readHomeDraft(draftKey) }))
      storeHomeContent()

      applyServerRetention(true)
      applyServerRetention(true)
      applyServerRetention(undefined)
      applyServerRetention(false)

      expect(heard).toEqual([
        { allowed: false, draft: '' },
        { allowed: true, draft: '' },
      ])
    })

    it('stops telling a subscriber that withdrew, and one that throws does not silence the rest', () => {
      const withdrawn = vi.fn()
      const heard = vi.fn()
      subscribe(withdrawn)()
      subscribe(() => {
        throw new Error('listener failed')
      })
      subscribe(heard)

      applyServerRetention(true)

      expect(withdrawn).not.toHaveBeenCalled()
      expect(heard).toHaveBeenCalledWith(false)
      expect(isLocalRetentionAllowed()).toBe(false)
    })
  })
})
