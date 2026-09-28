// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatSidebarPanel } from './CoreSidebar'
import { I18nProvider } from '@/shared/i18n'
import {
  CHAT_ACTIVE_THREAD_KEY,
  clearActiveChatThreadId,
  clearChatThreadHistory,
  readActiveChatThreadId,
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
}))

vi.mock('@/shared/api/chat-client', () => ({
  listChatThreads: chatClient.listChatThreads,
  saveChatThreadSnapshot: chatClient.saveChatThreadSnapshot,
  clearChatThreads: chatClient.clearChatThreads,
  deleteChatThread: chatClient.deleteChatThread,
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
