// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatSidebarPanel } from './CoreSidebar'
import type { AttentionSnapshot } from '@/shared/api/attention-client'
import { I18nProvider } from '@/shared/i18n'
import {
  clearActiveChatThreadId,
  clearChatThreadHistory,
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

function snapshot(items: AttentionSnapshot['items']): AttentionSnapshot {
  const count = (state: string) => items.filter((item) => item.state === state).length
  return {
    enabled: true,
    items,
    counts: { needsYou: count('needs_approval') + count('needs_input') + count('plan_ready'), working: count('working'), unseen: items.filter((item) => item.unseen).length },
    capabilities: { stream: false, push: false },
  }
}

function railRows(): string[] {
  const rail = document.querySelector('.chat-attention')
  return Array.from(rail?.querySelectorAll('.core-chat-session') ?? []).map((row) => row.textContent ?? '')
}

describe('ChatSidebarPanel attention rail', () => {
  beforeEach(() => {
    Object.defineProperty(window, 'localStorage', { configurable: true, value: createStorageMock() })
    Object.defineProperty(window, 'sessionStorage', { configurable: true, value: createStorageMock() })
    window.localStorage.clear()
    window.sessionStorage.clear()
    document.title = 'Verevon'

    upsertChatThreadHistory({ threadId: 'thread-old-approval', title: 'Old approval', preview: 'p', updatedAt: '2026-10-30T07:00:00.000Z' })
    upsertChatThreadHistory({ threadId: 'thread-new-approval', title: 'New approval', preview: 'p', updatedAt: '2026-10-30T10:00:00.000Z' })
    upsertChatThreadHistory({ threadId: 'thread-working', title: 'Running task', preview: 'p', updatedAt: '2026-10-30T09:00:00.000Z', latestRunStatus: 'running' })
    upsertChatThreadHistory({ threadId: 'thread-unread', title: 'Finished unread', preview: 'p', updatedAt: '2026-10-30T08:00:00.000Z' })
    upsertChatThreadHistory({ threadId: 'thread-read', title: 'Finished read', preview: 'p', updatedAt: '2026-10-30T06:00:00.000Z' })

    // Never resolves: these tests exercise the rail, not the server resync of the list.
    chatClient.listChatThreads.mockReset().mockReturnValue(new Promise(() => {}))
    chatClient.saveChatThreadSnapshot.mockReset().mockResolvedValue(null)
    chatClient.clearChatThreads.mockReset().mockResolvedValue(undefined)
    chatClient.deleteChatThread.mockReset().mockResolvedValue([])
  })

  afterEach(() => {
    cleanup()
    clearActiveChatThreadId()
    clearChatThreadHistory()
    attentionClient.getAttention.mockReset()
    document.title = ''
  })

  it('shows Waiting, Working and Unseen above the list, with the longest wait first and read chats out of the rail', async () => {
    attentionClient.getAttention.mockResolvedValue(snapshot([
      { threadId: 'thread-new-approval', state: 'needs_approval', reason: 'approval', since: '2026-10-30T09:30:00.000Z', unseen: false },
      { threadId: 'thread-old-approval', state: 'needs_approval', reason: 'approval', since: '2026-10-30T08:00:00.000Z', unseen: false },
      { threadId: 'thread-working', state: 'working', unseen: false },
      { threadId: 'thread-unread', state: 'completed', unseen: true },
      { threadId: 'thread-read', state: 'completed', unseen: false },
    ]))

    renderChatSidebar()
    const rail = await waitFor(() => {
      const section = document.querySelector('.chat-attention')
      expect(section).not.toBeNull()
      return section as HTMLElement
    })

    expect(rail.textContent).toContain('Venter på deg')
    expect(rail.textContent).toContain('Pågår')
    expect(rail.textContent).toContain('Ulest')
    expect(railRows().map((row) => row.match(/Old approval|New approval|Running task|Finished unread|Finished read/)?.[0])).toEqual([
      'Old approval',
      'New approval',
      'Running task',
      'Finished unread',
    ])
  })

  it('renders no section when nothing needs attention, and the list still shows every chat', async () => {
    attentionClient.getAttention.mockResolvedValue(snapshot([]))

    renderChatSidebar()
    await screen.findByText('Finished read')

    expect(document.querySelector('.chat-attention')).toBeNull()
    expect(screen.getByText('Old approval')).toBeTruthy()
  })

  it('renders no section and no count when the attention read fails', async () => {
    attentionClient.getAttention.mockRejectedValue(new Error('gateway down'))

    renderChatSidebar()
    await screen.findByText('Finished read')

    expect(document.querySelector('.chat-attention')).toBeNull()
    expect(document.title).toBe('Verevon')
  })

  it('counts distinct waiting threads in the tab title and announces only that count', async () => {
    attentionClient.getAttention.mockResolvedValue(snapshot([
      { threadId: 'thread-old-approval', state: 'needs_approval', unseen: false },
      { threadId: 'thread-new-approval', state: 'plan_ready', unseen: false },
      { threadId: 'thread-working', state: 'working', unseen: false },
    ]))

    renderChatSidebar()
    await screen.findByText('Venter på deg: 2')

    expect(document.title).toBe('(2) Verevon')
    expect(screen.getByText('Venter på deg: 2').getAttribute('aria-live')).toBe('polite')
  })

  it('keeps the run-status chip on a row until the legacy chip is retired, and reads it from the shared vocabulary', async () => {
    upsertChatThreadHistory({ threadId: 'thread-paused', title: 'Paused run', preview: 'p', updatedAt: '2026-10-30T05:00:00.000Z', latestRunStatus: 'paused' })
    attentionClient.getAttention.mockResolvedValue(snapshot([]))

    renderChatSidebar()
    const row = await screen.findByText('Paused run')

    expect(row.closest('.core-chat-session')?.textContent).toContain('Trenger oppmerksomhet')
  })
})
