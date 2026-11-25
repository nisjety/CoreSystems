// @vitest-environment jsdom

// WS07-S6: the sidebar's thread menu offers "Memory on this thread" for a chat thread, with the switch on. A write
// that the server refuses puts the stored mode back and shows the notice.

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatSidebarPanel } from './CoreSidebar'
import type { AttentionSnapshot } from '@/shared/api/attention-client'
import { I18nProvider } from '@/shared/i18n'
import {
  clearActiveChatThreadId,
  clearChatThreadHistory,
  readChatThreadHistory,
  upsertChatThreadHistory,
} from '@/features/chat/lib/chat-thread-history'

const attentionClient = vi.hoisted(() => ({
  getAttention: vi.fn(),
}))

const chatClient = vi.hoisted(() => ({
  listChatThreads: vi.fn(),
  saveChatThreadSnapshot: vi.fn(),
  clearChatThreads: vi.fn(),
  deleteChatThread: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', () => ({
  listChatThreads: chatClient.listChatThreads,
  saveChatThreadSnapshot: chatClient.saveChatThreadSnapshot,
  clearChatThreads: chatClient.clearChatThreads,
  deleteChatThread: chatClient.deleteChatThread,
}))

vi.mock('@/shared/api/attention-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/attention-client')>()),
  getAttention: attentionClient.getAttention,
}))

const MEMORY_LABEL = 'Minne på denne samtalen'

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

function renderChatSidebar() {
  const TestRouter = createRouter({
    routes: [{ path: '/chat', component: () => <ChatSidebarPanel onCollapse={() => undefined} /> }],
    history: memoryHistory('/chat'),
    explicitLinks: true,
  })
  return render(() => (
    <I18nProvider>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </I18nProvider>
  ))
}

const emptyAttention: AttentionSnapshot = {
  enabled: true,
  items: [],
  counts: { needsYou: 0, working: 0, unseen: 0 },
  capabilities: { stream: false, push: false },
} as AttentionSnapshot

async function openMenu(title: string) {
  fireEvent.click(await waitFor(() => screen.getByRole('button', { name: `Flere valg for ${title}` })))
  return waitFor(() => screen.getByRole('menu'))
}

describe('ChatSidebarPanel — memory on this thread (WS07-S6)', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', { configurable: true, value: createStorageMock() })
    Object.defineProperty(window, 'sessionStorage', { configurable: true, value: createStorageMock() })
    window.localStorage.clear()
    window.sessionStorage.clear()
    document.title = 'Verevon'

    // Never resolves: the list stays the local one these tests seeded.
    chatClient.listChatThreads.mockReset().mockReturnValue(new Promise(() => {}))
    chatClient.saveChatThreadSnapshot.mockReset().mockResolvedValue(null)
    chatClient.clearChatThreads.mockReset().mockResolvedValue(undefined)
    chatClient.deleteChatThread.mockReset().mockResolvedValue([])
    attentionClient.getAttention.mockReset().mockResolvedValue(emptyAttention)
  })

  afterEach(() => {
    cleanup()
    clearActiveChatThreadId()
    clearChatThreadHistory()
    vi.unstubAllEnvs()
    document.title = ''
  })

  it('offers the control on a chat thread when the switch is on, and reads it as on for a thread that inherits', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_MEMORY_MODE', 'true')
    upsertChatThreadHistory({ threadId: 'thread-chat', title: 'Ukentlig plan', preview: 'p', updatedAt: '2026-11-20T10:00:00.000Z', origin: 'chat' })

    renderChatSidebar()
    await openMenu('Ukentlig plan')

    const control = await waitFor(() => screen.getByRole('menuitemcheckbox', { name: MEMORY_LABEL }))
    expect(control.getAttribute('aria-checked')).toBe('true')
  })

  it('writes the other mode through saveChatThreadSnapshot and keeps it locally', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_MEMORY_MODE', 'true')
    upsertChatThreadHistory({ threadId: 'thread-chat', title: 'Ukentlig plan', preview: 'p', updatedAt: '2026-11-20T10:00:00.000Z', origin: 'chat' })

    renderChatSidebar()
    await openMenu('Ukentlig plan')
    fireEvent.click(await waitFor(() => screen.getByRole('menuitemcheckbox', { name: MEMORY_LABEL })))

    await waitFor(() => expect(chatClient.saveChatThreadSnapshot).toHaveBeenCalledTimes(1))
    expect(chatClient.saveChatThreadSnapshot).toHaveBeenCalledWith('thread-chat', { memoryMode: 'off' })
    expect(readChatThreadHistory()[0]?.memoryMode).toBe('off')
  })

  it('puts the stored mode back when the server refuses the write', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_MEMORY_MODE', 'true')
    chatClient.saveChatThreadSnapshot.mockReset().mockRejectedValue(new Error('refused'))
    upsertChatThreadHistory({ threadId: 'thread-chat', title: 'Ukentlig plan', preview: 'p', updatedAt: '2026-11-20T10:00:00.000Z', origin: 'chat' })

    renderChatSidebar()
    await openMenu('Ukentlig plan')
    fireEvent.click(await waitFor(() => screen.getByRole('menuitemcheckbox', { name: MEMORY_LABEL })))

    await waitFor(() => expect(readChatThreadHistory()[0]?.memoryMode).toBe('inherit'))
    expect(chatClient.saveChatThreadSnapshot).toHaveBeenCalledWith('thread-chat', { memoryMode: 'off' })
  })

  it('hides the control while the switch is off', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_MEMORY_MODE', '')
    upsertChatThreadHistory({ threadId: 'thread-chat', title: 'Ukentlig plan', preview: 'p', updatedAt: '2026-11-20T10:00:00.000Z', origin: 'chat' })

    renderChatSidebar()
    await openMenu('Ukentlig plan')

    expect(screen.getByRole('menuitem', { name: 'Gi nytt navn' })).toBeTruthy()
    expect(screen.queryByRole('menuitemcheckbox', { name: MEMORY_LABEL })).toBeNull()
  })

  it('hides the control on a thread another surface created', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_MEMORY_MODE', 'true')
    upsertChatThreadHistory({ threadId: 'thread-mail', title: 'Fra e-post-tråd', preview: 'p', updatedAt: '2026-11-20T10:00:00.000Z', origin: 'mail_task' })

    renderChatSidebar()
    await openMenu('Fra e-post-tråd')

    expect(screen.getByRole('menuitem', { name: 'Gi nytt navn' })).toBeTruthy()
    expect(screen.queryByRole('menuitemcheckbox', { name: MEMORY_LABEL })).toBeNull()
  })
})
