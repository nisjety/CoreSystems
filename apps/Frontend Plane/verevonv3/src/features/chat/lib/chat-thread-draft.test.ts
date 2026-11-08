// @vitest-environment jsdom
import { createRoot, createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const identity = vi.hoisted(() => ({ user: { id: 'user-a' } as { id: string } | null, activeOrg: { id: 'org-a' } as { id: string } | null }))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => identity }))

import { __resetRetentionForTests, applyServerRetention } from '@/features/chat/lib/chat-retention'
import { chatDraftStorageKey } from '@/shared/session/home-draft-storage'
import { CHAT_DRAFT_DEBOUNCE_MS, useChatThreadDraft, type ChatThreadDraft } from './chat-thread-draft'

const keyOf = (thread: string) => chatDraftStorageKey('user-a', 'org-a', thread)!
const keyA = keyOf('thread-a')
const keyB = keyOf('thread-b')
const keyNew = keyOf('new')

/** The text stored under `key`, or null when nothing is stored there. */
const stored = (key: string): string | null => {
  const raw = sessionStorage.getItem(key)
  return raw === null ? null : (JSON.parse(raw) as { text: string }).text
}

type Mounted = {
  draft: ChatThreadDraft
  /** Moves the chat to another thread, or to a new chat (`null`), the way the controller does. */
  go: (thread: string | null) => void
  /** Switches Temporary chat on or off for the next turn. */
  temporary: (on: boolean) => void
  dispose: () => void
}

const disposers: Array<() => void> = []

/** Mounts the hook in a reactive root, as the chat page does. Mounting again is a reload. */
function mount(thread: string | null = 'thread-a', temporary = false): Mounted {
  let mounted!: Mounted
  createRoot((dispose) => {
    disposers.push(dispose)
    const [threadId, setThreadId] = createSignal<string | null>(thread)
    const [zeroRetention, setZeroRetention] = createSignal(temporary)
    mounted = {
      draft: useChatThreadDraft({ threadId, zeroRetention }),
      go: (next) => {
        setThreadId(next)
        flush()
      },
      temporary: (on) => {
        setZeroRetention(on)
        flush()
      },
      dispose,
    }
  })
  flush()
  return mounted
}

/** The person types: the composer's text reaches the hook. */
function type(page: Mounted, text: string): void {
  page.draft.setMessage(text)
  flush()
}

/** Lets the typing pause run out, so the draft is written. */
function pause(): void {
  vi.advanceTimersByTime(CHAT_DRAFT_DEBOUNCE_MS)
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  localStorage.clear()
  sessionStorage.clear()
  identity.user = { id: 'user-a' }
  identity.activeOrg = { id: 'org-a' }
})

afterEach(() => {
  for (const dispose of disposers.splice(0)) dispose()
  __resetRetentionForTests()
  vi.useRealTimers()
})

describe('the chat thread draft', () => {
  it('is restored on the thread it was typed in, after a reload', () => {
    const first = mount('thread-a')
    type(first, 'Lang melding til A')
    pause()
    expect(stored(keyA)).toBe('Lang melding til A')
    first.dispose()

    const reloaded = mount('thread-a')
    expect(reloaded.draft.message()).toBe('Lang melding til A')
  })

  it('keeps a draft for each thread when the person switches between them', () => {
    const page = mount('thread-a')
    type(page, 'Utkast A')
    page.go('thread-b')
    expect(page.draft.message()).toBe('')
    type(page, 'Utkast B')

    page.go('thread-a')
    expect(page.draft.message()).toBe('Utkast A')
    page.go('thread-b')
    expect(page.draft.message()).toBe('Utkast B')
    expect(stored(keyA)).toBe('Utkast A')
    expect(stored(keyB)).toBe('Utkast B')
  })

  it('keeps the text of a send in storage until the send settles, and gives it back when the send fails', () => {
    const page = mount('thread-a')
    type(page, 'Viktig melding')
    const settle = page.draft.beginSend()
    type(page, '')
    expect(stored(keyA)).toBe('Viktig melding')

    settle('failed')
    flush()
    expect(page.draft.message()).toBe('Viktig melding')
    pause()
    expect(stored(keyA)).toBe('Viktig melding')
  })

  it('discards the draft of a send once the turn is accepted', () => {
    const page = mount('thread-a')
    type(page, 'Sendt melding')
    pause()
    const settle = page.draft.beginSend()
    type(page, '')

    settle('accepted')
    flush()
    expect(page.draft.message()).toBe('')
    expect(sessionStorage.getItem(keyA)).toBeNull()
  })

  it('keeps text typed after the send started, when the turn is accepted', () => {
    const page = mount('thread-a')
    type(page, 'Første')
    const settle = page.draft.beginSend()
    type(page, '')
    type(page, 'Neste')
    pause()

    settle('accepted')
    flush()
    expect(page.draft.message()).toBe('Neste')
    expect(stored(keyA)).toBe('Neste')
  })

  it('does not give a failed send back to a thread the person has left', () => {
    const page = mount('thread-a')
    type(page, 'Melding fra A')
    const settle = page.draft.beginSend()
    type(page, '')

    page.go('thread-b')
    settle('failed')
    flush()
    expect(page.draft.message()).toBe('')
    expect(stored(keyA)).toBe('Melding fra A')
  })

  it('starts a new chat with an empty composer and keeps the draft of the thread it leaves', () => {
    const page = mount('thread-a')
    type(page, 'Halvferdig')
    pause()

    page.draft.reset()
    page.go(null)
    expect(page.draft.message()).toBe('')
    expect(stored(keyA)).toBe('Halvferdig')
    expect(sessionStorage.getItem(keyNew)).toBeNull()
  })

  it('moves a new chat\'s draft to the thread the conversation takes', () => {
    const page = mount(null)
    type(page, 'Ny chat')
    pause()
    expect(stored(keyNew)).toBe('Ny chat')

    page.draft.carry(null, 'thread-a')
    page.go('thread-a')
    expect(sessionStorage.getItem(keyNew)).toBeNull()
    expect(stored(keyA)).toBe('Ny chat')
    expect(page.draft.message()).toBe('Ny chat')
  })

  it('gives a failed first send back to the thread the new chat became', () => {
    const page = mount(null)
    type(page, 'Første melding')
    page.draft.carry(null, 'thread-a')
    const settle = page.draft.beginSend()
    type(page, '')

    page.go('thread-a')
    expect(page.draft.message()).toBe('')
    expect(stored(keyA)).toBe('Første melding')
    settle('failed')
    flush()
    expect(page.draft.message()).toBe('Første melding')
  })

  it('writes nothing while Temporary chat is on', () => {
    const page = mount('thread-a', true)
    type(page, 'Privat')
    pause()
    expect(sessionStorage.length).toBe(0)
    expect(page.draft.message()).toBe('Privat')
  })

  it('removes the stored draft when Temporary chat is switched on, and does not store the text again when it is switched off', () => {
    const page = mount('thread-a')
    type(page, 'Lagret først')
    pause()
    expect(stored(keyA)).toBe('Lagret først')

    page.temporary(true)
    expect(sessionStorage.getItem(keyA)).toBeNull()
    page.temporary(false)
    pause()
    expect(sessionStorage.getItem(keyA)).toBeNull()
    expect(page.draft.message()).toBe('Lagret først')
  })

  it('removes stored drafts when the workspace denies local copies, and writes none while it does', () => {
    const page = mount('thread-a')
    type(page, 'Arbeidsdata')
    pause()
    expect(stored(keyA)).toBe('Arbeidsdata')

    applyServerRetention(true)
    flush()
    expect(sessionStorage.getItem(keyA)).toBeNull()
    type(page, 'Mer arbeidsdata')
    pause()
    expect(sessionStorage.getItem(keyA)).toBeNull()
  })

  it('keeps a failed send in Temporary chat in the composer only, never in storage', () => {
    const page = mount('thread-a', true)
    type(page, 'Kun på skjermen')
    const settle = page.draft.beginSend()
    type(page, '')

    settle('failed')
    flush()
    expect(page.draft.message()).toBe('Kun på skjermen')
    pause()
    expect(sessionStorage.length).toBe(0)
  })
})
