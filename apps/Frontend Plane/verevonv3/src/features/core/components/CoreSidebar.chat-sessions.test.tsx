// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatSidebarPanel } from './CoreSidebar'
import { I18nProvider } from '@/shared/i18n'
import { ATTENTION_CHANGED_EVENT } from '@/shared/api/attention-client'
import {
  CHAT_ACTIVE_THREAD_KEY,
  CHAT_SIDEBAR_MIGRATED_KEY,
  clearActiveChatThreadId,
  clearChatThreadHistory,
  readActiveChatThreadId,
  readChatThreadHistory,
  readChatThreadTranscript,
  setActiveChatThreadId,
  upsertChatThreadHistory,
  upsertChatThreadTranscript,
} from '@/features/chat/lib/chat-thread-history'

const chatClient = vi.hoisted(() => ({
  listChatThreads: vi.fn(),
  saveChatThreadSnapshot: vi.fn(),
  clearChatThreads: vi.fn(),
  deleteChatThread: vi.fn(),
  listChatThreadsPage: vi.fn(),
  getChatThread: vi.fn(),
  searchChatThreads: vi.fn(),
  unarchiveChatThread: vi.fn(),
}))

vi.mock('@/shared/api/chat-client', () => ({
  listChatThreads: chatClient.listChatThreads,
  saveChatThreadSnapshot: chatClient.saveChatThreadSnapshot,
  clearChatThreads: chatClient.clearChatThreads,
  deleteChatThread: chatClient.deleteChatThread,
  listChatThreadsPage: chatClient.listChatThreadsPage,
  getChatThread: chatClient.getChatThread,
  searchChatThreads: chatClient.searchChatThreads,
  unarchiveChatThread: chatClient.unarchiveChatThread,
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

function renderChatSidebar(onCollapse: () => void = () => undefined) {
  const TestRouter = createRouter({
    routes: [{ path: '/chat', component: () => <ChatSidebarPanel onCollapse={onCollapse} /> }],
    history: memoryHistory('/chat'),
    explicitLinks: true,
  })
  return render(() => (
    <I18nProvider>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </I18nProvider>
  ))
}

describe('ChatSidebarPanel row actions (rename / delete)', () => {
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

    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First conversation',
      preview: 'Preview one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second conversation',
      preview: 'Preview two',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    // Never resolves: these tests exercise one row's interactions, not the
    // panel's onMount server resync, and a resolving listing would replace
    // `sessions` with freshly-parsed (referentially different) objects at an
    // unpredictable point in the middle of a test.
    chatClient.listChatThreads.mockReset().mockReturnValue(new Promise(() => {}))
    chatClient.saveChatThreadSnapshot.mockReset().mockResolvedValue(null)
    chatClient.clearChatThreads.mockReset().mockResolvedValue(undefined)
    chatClient.deleteChatThread.mockReset().mockResolvedValue([])
  })

  afterEach(() => {
    cleanup()
    clearActiveChatThreadId()
    clearChatThreadHistory()
  })

  it('opens a row menu from its trigger and focuses Rename first', async () => {
    renderChatSidebar()
    await screen.findByText('First conversation')

    const trigger = screen.getByRole('button', { name: /flere valg for first conversation/i })
    fireEvent.click(trigger)
    flush()

    expect(trigger.getAttribute('aria-expanded')).toBe('true')
    const renameItem = screen.getByRole('menuitem', { name: /gi nytt navn/i })
    await waitFor(() => expect(document.activeElement).toBe(renameItem))
  })

  it('renames a conversation on Enter, saving only the title through save_thread', async () => {
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /gi nytt navn/i }))
    flush()

    const input = screen.getByRole('textbox', { name: /nytt navn på samtalen/i }) as HTMLInputElement
    fireEvent.input(input, { target: { value: '  Renamed conversation  ' } })
    // A signal write from inside a DOM event handler is only visible to a
    // LATER event's handler once `flush()` runs -- without this, the
    // `onSubmit` fired next would still read the pre-input `renameDraft`.
    flush()
    fireEvent.submit(input.closest('form')!)
    flush()

    expect(screen.getByText('Renamed conversation')).toBeTruthy()
    expect(screen.queryByText('First conversation')).toBeNull()
    // Trimmed before it ever reaches the wire.
    await waitFor(() => expect(chatClient.saveChatThreadSnapshot).toHaveBeenCalledWith('thread-1', { title: 'Renamed conversation' }))
    // Never touches pin/preview -- `save_thread` treats an absent field as
    // "leave as is", so sending anything else here would risk clobbering it.
    expect(chatClient.saveChatThreadSnapshot.mock.calls[0]?.[1]).toEqual({ title: 'Renamed conversation' })
  })

  it('treats an empty rename submission as a cancel rather than a save', async () => {
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /gi nytt navn/i }))
    flush()

    const input = screen.getByRole('textbox', { name: /nytt navn på samtalen/i }) as HTMLInputElement
    fireEvent.input(input, { target: { value: '   ' } })
    flush()
    fireEvent.submit(input.closest('form')!)
    flush()

    expect(screen.getByText('First conversation')).toBeTruthy()
    expect(chatClient.saveChatThreadSnapshot).not.toHaveBeenCalled()
  })

  it('cancels an in-progress rename on Escape without saving', async () => {
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /gi nytt navn/i }))
    flush()

    const input = screen.getByRole('textbox', { name: /nytt navn på samtalen/i })
    fireEvent.input(input, { target: { value: 'Should not save' } })
    flush()
    fireEvent.keyDown(input, { key: 'Escape' })
    flush()

    expect(screen.getByText('First conversation')).toBeTruthy()
    expect(screen.queryByText('Should not save')).toBeNull()
    expect(chatClient.saveChatThreadSnapshot).not.toHaveBeenCalled()
  })

  it('reverts the title and reports an error when the server rename fails', async () => {
    chatClient.saveChatThreadSnapshot.mockRejectedValueOnce(new Error('network'))
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /gi nytt navn/i }))
    flush()

    const input = screen.getByRole('textbox', { name: /nytt navn på samtalen/i }) as HTMLInputElement
    fireEvent.input(input, { target: { value: 'Renamed conversation' } })
    flush()
    fireEvent.submit(input.closest('form')!)
    flush()

    // The rejected `saveChatThreadSnapshot` settles on a later microtask, and
    // the resulting `setError`/revert only reaches the DOM once `flush()`
    // runs -- `waitFor` polls the DOM itself, so flushing on every attempt is
    // what lets it observe an update Solid hasn't committed yet.
    await waitFor(() => {
      flush()
      expect(screen.getByText('First conversation')).toBeTruthy()
      expect(screen.getByText(/kunne ikke endre navn/i)).toBeTruthy()
    })
    expect(screen.queryByText('Renamed conversation')).toBeNull()
  })

  it('toggles the per-thread persona switch and saves only personaEnabled through save_thread', async () => {
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()

    // Absent from local history == enabled by default (see
    // `withPersonaEnabledCarry`), so the menu item's label offers to turn it
    // off.
    const toggle = screen.getByRole('menuitemcheckbox', { name: /slå av persona for denne samtalen/i })
    expect(toggle.getAttribute('aria-checked')).toBe('true')
    fireEvent.click(toggle)
    flush()

    await waitFor(() => expect(chatClient.saveChatThreadSnapshot).toHaveBeenCalledWith('thread-1', { personaEnabled: false }))
    // Never touches title/preview/pin -- same "leave as is" contract the
    // rename test above already asserts for its own field.
    expect(chatClient.saveChatThreadSnapshot.mock.calls[0]?.[1]).toEqual({ personaEnabled: false })

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    const flipped = screen.getByRole('menuitemcheckbox', { name: /slå på persona for denne samtalen/i })
    expect(flipped.getAttribute('aria-checked')).toBe('false')
  })

  it('reverts the persona toggle and reports an error when the server write fails', async () => {
    chatClient.saveChatThreadSnapshot.mockRejectedValueOnce(new Error('network'))
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: /slå av persona for denne samtalen/i }))
    flush()

    await waitFor(() => {
      flush()
      expect(screen.getByText(/kunne ikke endre persona/i)).toBeTruthy()
    })
    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    expect(screen.getByRole('menuitemcheckbox', { name: /slå av persona for denne samtalen/i }).getAttribute('aria-checked')).toBe('true')
  })

  it('deletes a conversation only after the explicit confirm step, and leaves the user on a fresh chat', async () => {
    setActiveChatThreadId('thread-1')
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()

    const deleteButton = screen.getByRole('menuitem', { name: /^slett$/i })
    fireEvent.click(deleteButton)
    flush()

    // First click only arms the confirm -- nothing is deleted yet.
    expect(screen.getByText('First conversation')).toBeTruthy()
    expect(chatClient.deleteChatThread).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('menuitem', { name: /bekreft sletting/i }))
    flush()

    expect(screen.queryByText('First conversation')).toBeNull()
    // The active pointer is cleared -- `use-chat-controller` resets the chat
    // view itself off the same event this dispatches, so the reader lands on
    // a fresh chat rather than a dead thread.
    expect(window.localStorage.getItem(CHAT_ACTIVE_THREAD_KEY)).toBeNull()
    await waitFor(() => expect(chatClient.deleteChatThread).toHaveBeenCalledWith('thread-1'))
  })

  it('restores the row and reports an error when the server delete fails', async () => {
    chatClient.deleteChatThread.mockRejectedValueOnce(new Error('network'))
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /^slett$/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /bekreft sletting/i }))
    flush()

    await waitFor(() => {
      flush()
      expect(screen.getByText('First conversation')).toBeTruthy()
      expect(screen.getByText(/kunne ikke slette/i)).toBeTruthy()
    })
  })

  it('restores the active pointer and cached transcript when deleting the OPEN thread fails on the server', async () => {
    // The thread being deleted is also the one currently open, and it has a
    // cached transcript -- both of those are dropped by the optimistic
    // removal (`removeChatThreadHistoryItem` clears the transcript cache as a
    // side effect) and must come back once the server rejects the delete,
    // not just the history row.
    setActiveChatThreadId('thread-1')
    upsertChatThreadTranscript({
      threadId: 'thread-1',
      turns: [{ id: 'user-1', role: 'user', content: 'hello', createdAt: '2026-06-17T10:00:00.000Z' }],
    })
    chatClient.deleteChatThread.mockRejectedValueOnce(new Error('network'))
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /^slett$/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /bekreft sletting/i }))
    flush()

    // Optimistic removal already navigated away -- confirm the pointer was
    // really cleared before the (failing) request settles.
    expect(readActiveChatThreadId()).toBeNull()

    await waitFor(() => {
      flush()
      expect(screen.getByText('First conversation')).toBeTruthy()
      expect(screen.getByText(/kunne ikke slette/i)).toBeTruthy()
    })
    expect(readActiveChatThreadId()).toBe('thread-1')
    expect(readChatThreadTranscript('thread-1')?.turns).toHaveLength(1)
  })

  it('closes the row menu on Escape and returns focus to its trigger', async () => {
    renderChatSidebar()
    await screen.findByText('First conversation')

    const trigger = screen.getByRole('button', { name: /flere valg for first conversation/i })
    fireEvent.click(trigger)
    flush()
    expect(screen.getByRole('menu')).toBeTruthy()

    fireEvent.keyDown(window, { key: 'Escape' })
    flush()

    expect(screen.queryByRole('menu')).toBeNull()
    expect(document.activeElement).toBe(trigger)
  })

  it('closes the row menu on an outside click without acting on either row', async () => {
    renderChatSidebar()
    await screen.findByText('First conversation')

    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    expect(screen.getByRole('menu')).toBeTruthy()

    fireEvent.pointerDown(document.body)
    flush()

    expect(screen.queryByRole('menu')).toBeNull()
    expect(screen.getByText('First conversation')).toBeTruthy()
    expect(chatClient.deleteChatThread).not.toHaveBeenCalled()
  })
})

describe('ChatSidebarPanel mail-task origin badge', () => {
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

    chatClient.listChatThreads.mockReset().mockResolvedValue([
      {
        threadId: 'thread-mail-task',
        title: 'From a forwarded email',
        preview: 'Preview',
        updatedAt: '2026-06-17T09:00:00.000Z',
        pinned: false,
        origin: 'mail_task',
      },
      {
        threadId: 'thread-ordinary',
        title: 'Ordinary chat',
        preview: 'Preview',
        updatedAt: '2026-06-17T08:00:00.000Z',
        pinned: false,
      },
    ])
    chatClient.saveChatThreadSnapshot.mockReset().mockResolvedValue(null)
    chatClient.clearChatThreads.mockReset().mockResolvedValue(undefined)
    chatClient.deleteChatThread.mockReset().mockResolvedValue([])
  })

  afterEach(() => {
    cleanup()
    clearActiveChatThreadId()
    clearChatThreadHistory()
  })

  it('shows the "Fra e-post" badge only on the thread with origin=mail_task', async () => {
    renderChatSidebar()
    await screen.findByText('From a forwarded email')
    await screen.findByText('Ordinary chat')

    const mailTaskRow = screen.getByText('From a forwarded email').closest('.core-chat-session')
    const ordinaryRow = screen.getByText('Ordinary chat').closest('.core-chat-session')

    expect(mailTaskRow?.textContent).toContain('Fra e-post')
    expect(ordinaryRow?.textContent).not.toContain('Fra e-post')
  })
})

describe('ChatSidebarPanel run-status chip', () => {
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

    upsertChatThreadHistory({
      threadId: 'thread-running',
      title: 'Running chat',
      preview: 'Preview',
      updatedAt: '2026-06-17T09:00:00.000Z',
      latestRunStatus: 'running',
    })
    upsertChatThreadHistory({
      threadId: 'thread-failed',
      title: 'Failed chat',
      preview: 'Preview',
      updatedAt: '2026-06-17T08:00:00.000Z',
      latestRunStatus: 'timed_out',
    })
    upsertChatThreadHistory({
      threadId: 'thread-done',
      title: 'Completed chat',
      preview: 'Preview',
      updatedAt: '2026-06-17T07:00:00.000Z',
      latestRunStatus: 'completed',
    })

    chatClient.listChatThreads.mockReset().mockReturnValue(new Promise(() => {}))
    chatClient.saveChatThreadSnapshot.mockReset().mockResolvedValue(null)
    chatClient.clearChatThreads.mockReset().mockResolvedValue(undefined)
    chatClient.deleteChatThread.mockReset().mockResolvedValue([])
  })

  afterEach(() => {
    cleanup()
    clearActiveChatThreadId()
    clearChatThreadHistory()
  })

  it('labels an active or failed run from the shared attention states and shows no chip for a completed one', async () => {
    renderChatSidebar()
    await screen.findByText('Running chat')

    const chipText = (title: string) => screen.getByText(title).closest('.core-chat-session')?.textContent ?? ''
    expect(chipText('Running chat')).toContain('Pågår')
    expect(chipText('Failed chat')).toContain('Mislyktes')
    expect(chipText('Completed chat')).not.toContain('Pågår')
    expect(chipText('Completed chat')).not.toContain('Mislyktes')
  })

  it('shows the server attention state on a row that carries one, ahead of its run status', async () => {
    upsertChatThreadHistory({
      threadId: 'thread-server-working',
      title: 'Server working chat',
      preview: 'Preview',
      updatedAt: '2026-06-17T10:00:00.000Z',
      latestRunStatus: 'awaiting_approval',
      attentionState: 'working',
    })
    renderChatSidebar()
    await screen.findByText('Server working chat')

    const chipText = (title: string) => screen.getByText(title).closest('.core-chat-session')?.textContent ?? ''
    expect(chipText('Server working chat')).toContain('Pågår')
    expect(chipText('Server working chat')).not.toContain('Trenger oppmerksomhet')
  })
})

describe('ChatSidebarPanel server paging and thread state', () => {
  const serverSession = (threadId: string, title: string, extra: Record<string, unknown> = {}) => ({
    threadId,
    title,
    preview: 'Server preview',
    updatedAt: '2026-06-18T09:00:00.000Z',
    pinned: false,
    personaEnabled: true,
    ...extra,
  })

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

    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First conversation',
      preview: 'Preview one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second conversation',
      preview: 'Preview two',
      updatedAt: '2026-06-17T10:00:00.000Z',
    })

    // Never resolves unless a test says otherwise, as in the row-action suite above.
    chatClient.listChatThreads.mockReset().mockReturnValue(new Promise(() => {}))
    chatClient.listChatThreadsPage.mockReset().mockResolvedValue({ sessions: [], nextCursor: null })
    chatClient.getChatThread.mockReset().mockResolvedValue(null)
    chatClient.searchChatThreads.mockReset()
    chatClient.saveChatThreadSnapshot.mockReset().mockResolvedValue(null)
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllEnvs()
    clearActiveChatThreadId()
    clearChatThreadHistory()
  })

  it('reads the whole listing once, saves the local chats the server does not list, then records the move', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    chatClient.listChatThreads.mockResolvedValue([serverSession('thread-1', 'First conversation')])
    renderChatSidebar()

    await waitFor(() => expect(window.localStorage.getItem(CHAT_SIDEBAR_MIGRATED_KEY)).toBe('true'))
    expect(chatClient.listChatThreads).toHaveBeenCalled()
    expect(chatClient.listChatThreadsPage).not.toHaveBeenCalled()
    // The local chat the server did not list is saved first, so the paged listing cannot leave it behind.
    expect(chatClient.saveChatThreadSnapshot).toHaveBeenCalledWith('thread-2', expect.objectContaining({ title: 'Second conversation' }))
  })

  it('loads one server page once the move is recorded, and Load more appends without repeating a chat', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    window.localStorage.setItem(CHAT_SIDEBAR_MIGRATED_KEY, 'true')
    chatClient.listChatThreadsPage.mockImplementation(async (options: { cursor?: string } = {}) => (
      options.cursor === 'cursor-2'
        ? { sessions: [serverSession('thread-3', 'Third conversation'), serverSession('thread-1', 'First conversation')], nextCursor: null }
        : { sessions: [serverSession('thread-1', 'First conversation'), serverSession('thread-2', 'Second conversation')], nextCursor: 'cursor-2' }
    ))
    renderChatSidebar()

    await waitFor(() => expect(chatClient.listChatThreadsPage).toHaveBeenCalledWith({}))
    expect(chatClient.listChatThreadsPage).toHaveBeenCalledTimes(1)
    expect(chatClient.listChatThreads).not.toHaveBeenCalled()

    fireEvent.click(await screen.findByRole('button', { name: /last flere samtaler/i }))
    await screen.findByText('Third conversation')
    expect(chatClient.listChatThreadsPage).toHaveBeenLastCalledWith({ cursor: 'cursor-2' })
    expect(screen.getAllByText('First conversation')).toHaveLength(1)
    expect(screen.queryByRole('button', { name: /last flere samtaler/i })).toBeNull()
  })

  it('replaces the cache with the first page alone, so a local chat past it is not merged back', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    window.localStorage.setItem(CHAT_SIDEBAR_MIGRATED_KEY, 'true')
    chatClient.listChatThreadsPage.mockResolvedValue({ sessions: [serverSession('thread-3', 'Third conversation')], nextCursor: null })
    renderChatSidebar()

    await screen.findByText('Third conversation')
    expect(screen.queryByText('First conversation')).toBeNull()
    expect(screen.queryByText('Second conversation')).toBeNull()
    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['thread-3'])
  })

  it('keeps the whole-listing path with the switch off, whatever the stored move says', async () => {
    window.localStorage.setItem(CHAT_SIDEBAR_MIGRATED_KEY, 'true')
    chatClient.listChatThreads.mockResolvedValue([serverSession('thread-1', 'First conversation')])
    renderChatSidebar()

    await waitFor(() => expect(chatClient.saveChatThreadSnapshot).toHaveBeenCalledWith('thread-2', expect.anything()))
    expect(chatClient.listChatThreads).toHaveBeenCalled()
    expect(chatClient.listChatThreadsPage).not.toHaveBeenCalled()
  })

  it('shows the server unread dot and the Starred filter only with the thread-state switch on', async () => {
    // Seeded with the switch on, so the cache holds the flags the switch-off render has to ignore.
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second conversation',
      preview: 'Preview two',
      updatedAt: '2026-06-17T10:00:00.000Z',
      starred: true,
      unread: true,
    })
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'false')
    renderChatSidebar()
    await screen.findByText('Second conversation')
    expect(screen.queryByRole('img', { name: /ulest svar/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /stjernemerkede/i })).toBeNull()
    cleanup()

    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    renderChatSidebar()
    await screen.findByText('Second conversation')
    expect(screen.getByRole('img', { name: /ulest svar/i }).closest('.chat-organizer__draggable')?.textContent).toContain('Second conversation')
    expect(screen.getByRole('button', { name: /stjernemerkede/i })).toBeTruthy()
  })

  const searchHit = {
    threadId: 'thread-2',
    messageId: 'message-1',
    role: 'assistant',
    snippet: 'the receipt is filed',
    createdAt: '2026-06-17T10:00:00.000Z',
    threadTitle: 'Second conversation',
    archived: false,
  }

  async function searchAndPickSecondConversation() {
    chatClient.searchChatThreads.mockResolvedValue({ status: 'ok', hits: [searchHit] })
    renderChatSidebar()
    fireEvent.click(await screen.findByRole('button', { name: /søk i samtaler/i }))
    flush()
    fireEvent.input(screen.getByRole('combobox', { name: /søk i samtaler/i }), { target: { value: 'receipt' } })
    fireEvent.click(await screen.findByRole('option', { name: /second conversation/i }))
  }

  it('opens a search hit in the sidebar with the thread-state switch on', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    await searchAndPickSecondConversation()
    expect(readActiveChatThreadId()).toBe('thread-2')
  })

  it('leaves a search hit unopened with the thread-state switch off', async () => {
    await searchAndPickSecondConversation()
    expect(readActiveChatThreadId()).toBeNull()
  })

  it('refreshes the open thread from the server on an attention change, with the thread-state switch on', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second conversation',
      preview: 'Preview two',
      updatedAt: '2026-06-17T10:00:00.000Z',
      unread: true,
    })
    setActiveChatThreadId('thread-2')
    chatClient.getChatThread.mockResolvedValue(serverSession('thread-2', 'Second conversation', { unread: false }))
    renderChatSidebar()
    await screen.findByRole('img', { name: /ulest svar/i })

    window.dispatchEvent(new Event(ATTENTION_CHANGED_EVENT))

    await waitFor(() => expect(screen.queryByRole('img', { name: /ulest svar/i })).toBeNull())
    expect(chatClient.getChatThread).toHaveBeenCalledWith('thread-2')
  })

  it('asks the server for its first page of starred chats when the Starred filter turns on', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
    upsertChatThreadHistory({
      threadId: 'thread-2',
      title: 'Second conversation',
      preview: 'Preview two',
      updatedAt: '2026-06-17T10:00:00.000Z',
      starred: true,
    })
    chatClient.listChatThreadsPage.mockResolvedValue({
      sessions: [serverSession('thread-2', 'Second conversation', { starred: true })],
      nextCursor: null,
    })
    renderChatSidebar()

    const toggle = await screen.findByRole('button', { name: /stjernemerkede/i })
    fireEvent.click(toggle)

    await waitFor(() => expect(chatClient.listChatThreadsPage).toHaveBeenCalledWith({ starredOnly: true }))
    expect(toggle.getAttribute('aria-pressed')).toBe('true')
    expect(await screen.findByText('Second conversation')).toBeTruthy()
  })

  it('drops the stored transcript of a chat the first page no longer holds, and keeps the open chat and the listed ones', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    window.localStorage.setItem(CHAT_SIDEBAR_MIGRATED_KEY, 'true')
    upsertChatThreadTranscript({ threadId: 'thread-1', turns: [], updatedAt: '2026-06-17T09:00:00.000Z' })
    upsertChatThreadTranscript({ threadId: 'thread-2', turns: [], updatedAt: '2026-06-17T10:00:00.000Z' })
    upsertChatThreadTranscript({ threadId: 'thread-3', turns: [], updatedAt: '2026-06-18T09:00:00.000Z' })
    setActiveChatThreadId('thread-2')
    chatClient.listChatThreadsPage.mockResolvedValue({
      sessions: [serverSession('thread-3', 'Third conversation')],
      nextCursor: null,
    })
    renderChatSidebar()

    await screen.findByText('Third conversation')
    // thread-1 left the cache with the first-page replace and is not open, so its conversation goes with it.
    expect(readChatThreadTranscript('thread-1')).toBeNull()
    expect(readChatThreadTranscript('thread-2')).not.toBeNull()
    expect(readChatThreadTranscript('thread-3')).not.toBeNull()
  })

  const archivedSession = serverSession('thread-9', 'Archived conversation', { updatedAt: '2026-06-10T09:00:00.000Z' })

  async function restoreArchivedConversation() {
    fireEvent.click(await screen.findByRole('button', { name: /vis arkiverte/i }))
    fireEvent.click(await screen.findByRole('button', { name: /gjenopprett archived conversation/i }))
    await waitFor(() => expect(chatClient.unarchiveChatThread).toHaveBeenCalledWith('thread-9'))
  }

  it('puts a restored chat into the paged list even when the first page does not hold it', async () => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_SERVER_PAGING', 'true')
    window.localStorage.setItem(CHAT_SIDEBAR_MIGRATED_KEY, 'true')
    chatClient.listChatThreadsPage.mockImplementation(async (options: { archived?: string } = {}) => (
      options.archived === 'archived'
        ? { sessions: [archivedSession], nextCursor: null }
        : { sessions: [serverSession('thread-3', 'Third conversation')], nextCursor: 'cursor-2' }
    ))
    chatClient.unarchiveChatThread.mockReset().mockResolvedValue(archivedSession)
    renderChatSidebar()
    await screen.findByText('Third conversation')

    await restoreArchivedConversation()
    fireEvent.click(await screen.findByRole('button', { name: /vis aktive samtaler/i }))

    expect(await screen.findByText('Archived conversation')).toBeTruthy()
    expect(readChatThreadHistory().map((item) => item.threadId)).toEqual(['thread-3', 'thread-9'])
    // No second first-page read, which would have dropped the restored chat again.
    expect(chatClient.listChatThreadsPage.mock.calls.filter(([options]) => !options?.archived)).toHaveLength(1)
    expect(screen.queryByText(/kunne ikke gjenopprette/i)).toBeNull()
  })

  it('reads the whole listing again after a restore with the switch off', async () => {
    chatClient.listChatThreads.mockResolvedValue([
      serverSession('thread-1', 'First conversation'),
      serverSession('thread-2', 'Second conversation'),
    ])
    chatClient.listChatThreadsPage.mockResolvedValue({ sessions: [archivedSession], nextCursor: null })
    chatClient.unarchiveChatThread.mockReset().mockResolvedValue(archivedSession)
    renderChatSidebar()
    await waitFor(() => expect(chatClient.listChatThreads).toHaveBeenCalledTimes(1))

    await restoreArchivedConversation()

    await waitFor(() => expect(chatClient.listChatThreads).toHaveBeenCalledTimes(2))
    expect(chatClient.listChatThreadsPage).not.toHaveBeenCalledWith({})
    expect(readChatThreadHistory().map((item) => item.threadId)).not.toContain('thread-9')
  })
})

describe('ChatSidebarPanel delete notice for forked chats (F-f05-23)', () => {
  beforeEach(() => {
    vi.stubEnv('VITE_CHAT_SIDEBAR_THREAD_STATE', 'true')
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

    // Never resolves, as in the row-action tests above: the server resync is not under test here.
    chatClient.listChatThreads.mockReset().mockReturnValue(new Promise(() => {}))
    chatClient.deleteChatThread.mockReset().mockResolvedValue([])
  })

  afterEach(() => {
    vi.unstubAllEnvs()
    cleanup()
    clearActiveChatThreadId()
    clearChatThreadHistory()
  })

  const base = 'Slettes for godt og kan ikke gjenopprettes.'

  /** Opens the first chat's menu and arms its Delete, which shows the consequence the confirmation states. */
  function armedDeleteConsequence(): string {
    fireEvent.click(screen.getByRole('button', { name: /flere valg for first conversation/i }))
    flush()
    fireEvent.click(screen.getByRole('menuitem', { name: /^slett$/i }))
    flush()
    return document.querySelector('.verevon-space-confirm__consequence')?.textContent ?? ''
  }

  it('says how many chats forked from it keep their own copy, while the fork switch is on', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'true')
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First conversation',
      preview: 'Preview one',
      updatedAt: '2026-06-17T09:00:00.000Z',
      forkCount: 2,
    })
    renderChatSidebar()
    await screen.findByText('First conversation')

    expect(armedDeleteConsequence()).toBe(`${base} Kopier som er forgrenet fra denne samtalen beholdes (2).`)
    // Deleting never cascades to the forks: the confirmation deletes this one chat.
    fireEvent.click(screen.getByRole('menuitem', { name: /bekreft sletting/i }))
    flush()
    await waitFor(() => expect(chatClient.deleteChatThread).toHaveBeenCalledWith('thread-1'))
    expect(chatClient.deleteChatThread).toHaveBeenCalledTimes(1)
  })

  it('uses the singular for a single forked chat', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'true')
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First conversation',
      preview: 'Preview one',
      updatedAt: '2026-06-17T09:00:00.000Z',
      forkCount: 1,
    })
    renderChatSidebar()
    await screen.findByText('First conversation')

    expect(armedDeleteConsequence()).toBe(`${base} Den forgrenede samtalen beholder sin egen kopi.`)
  })

  it('leaves the confirmation as it was for a chat nobody forked from', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'true')
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First conversation',
      preview: 'Preview one',
      updatedAt: '2026-06-17T09:00:00.000Z',
    })
    renderChatSidebar()
    await screen.findByText('First conversation')

    expect(armedDeleteConsequence()).toBe(base)
  })

  it('leaves the line out while the fork switch is off', async () => {
    vi.stubEnv('VITE_CHAT_THREAD_FORK', 'false')
    upsertChatThreadHistory({
      threadId: 'thread-1',
      title: 'First conversation',
      preview: 'Preview one',
      updatedAt: '2026-06-17T09:00:00.000Z',
      forkCount: 2,
    })
    renderChatSidebar()
    await screen.findByText('First conversation')

    expect(armedDeleteConsequence()).toBe(base)
  })
})
