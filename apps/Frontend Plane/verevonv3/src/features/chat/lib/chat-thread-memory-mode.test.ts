import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  chatThreadMemoryControlVisible,
  chatThreadMemoryModeEnabled,
  clearChatThreadHistory,
  readChatThreadHistory,
  replaceChatThreadHistory,
  setChatThreadMemoryModeLocally,
  toggleThreadMemoryMode,
  upsertChatThreadHistory,
} from '@/features/chat/lib/chat-thread-history'

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

const updatedAt = '2026-11-20T10:00:00.000Z'

describe('chat thread memory mode (WS07-S6)', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', { configurable: true, value: createStorageMock() })
    Object.defineProperty(window, 'sessionStorage', { configurable: true, value: createStorageMock() })
    window.localStorage.clear()
    window.sessionStorage.clear()
  })

  afterEach(() => {
    clearChatThreadHistory()
    vi.unstubAllEnvs()
  })

  it('moves a thread that follows the memory settings to off, and an off thread back to inherit', () => {
    expect(toggleThreadMemoryMode(undefined)).toBe('off')
    expect(toggleThreadMemoryMode('inherit')).toBe('off')
    expect(toggleThreadMemoryMode('off')).toBe('inherit')
  })

  it('reads the switch as the literal true only', () => {
    for (const value of ['', '1', 'TRUE', 'yes', 'false']) {
      vi.stubEnv('VITE_CHAT_THREAD_MEMORY_MODE', value)
      expect(chatThreadMemoryModeEnabled(), value).toBe(false)
    }
    vi.stubEnv('VITE_CHAT_THREAD_MEMORY_MODE', 'true')
    expect(chatThreadMemoryModeEnabled()).toBe(true)
  })

  it('offers the control only on a chat thread, with the switch on and local retention allowed', () => {
    const on = { enabled: true, localRetentionAllowed: true }
    expect(chatThreadMemoryControlVisible({ origin: 'chat' }, on)).toBe(true)
    expect(chatThreadMemoryControlVisible({ origin: 'chat' }, { enabled: false, localRetentionAllowed: true })).toBe(false)
    // A zero-retention posture hides it: the device may not keep the conversation's content.
    expect(chatThreadMemoryControlVisible({ origin: 'chat' }, { enabled: true, localRetentionAllowed: false })).toBe(false)
    for (const origin of ['mail_task', 'space', 'support', 'agent_run', undefined]) {
      expect(chatThreadMemoryControlVisible({ origin }, on), String(origin)).toBe(false)
    }
  })

  it('sets a thread mode locally, and a later local write that names no mode keeps it', () => {
    upsertChatThreadHistory({ threadId: 'thread-1', title: 'Plan', preview: 'p', updatedAt })

    setChatThreadMemoryModeLocally('thread-1', 'off')
    expect(readChatThreadHistory()[0]?.memoryMode).toBe('off')

    upsertChatThreadHistory({ threadId: 'thread-1', title: 'Renamed', preview: 'p', updatedAt })
    expect(readChatThreadHistory()[0]).toMatchObject({ title: 'Renamed', memoryMode: 'off' })
  })

  it('does nothing for a thread that has no history entry', () => {
    setChatThreadMemoryModeLocally('thread-missing', 'off')
    expect(readChatThreadHistory()).toEqual([])
  })

  it('keeps only a closed memory mode from a server list', () => {
    replaceChatThreadHistory([
      { threadId: 'thread-1', title: 'A', preview: 'p', updatedAt, memoryMode: 'off' },
      { threadId: 'thread-2', title: 'B', preview: 'p', updatedAt, memoryMode: 'bogus' as unknown as 'off' },
    ])

    const modes = Object.fromEntries(readChatThreadHistory().map((item) => [item.threadId, item.memoryMode]))
    expect(modes).toEqual({ 'thread-1': 'off', 'thread-2': undefined })
  })
})
