import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChatSidebarOrganizer } from './ChatSidebarOrganizer'
import { chatOrganizationStorageKey, emptyChatOrganization, readChatOrganization, saveChatOrganization } from '@/features/chat/lib/chat-organization'
import type { ChatSearchHit } from '@/shared/api/chat-client'
import { SEARCH_DEBOUNCE_MS } from './ChatSearchResults'

const { searchChatThreads } = vi.hoisted(() => ({ searchChatThreads: vi.fn() }))
vi.mock('@/shared/api/chat-client', () => ({ searchChatThreads }))
// The real error copy seam (translateApiError, LocalizedError); only the hook is faked.
vi.mock('@/shared/i18n', async () => ({
  ...(await vi.importActual<typeof import('@/shared/i18n/errors')>('@/shared/i18n/errors')),
  useI18n: () => ({ tr: (_no: string, en: string) => en }),
}))
vi.mock('@/shared/session/session-store', () => ({ getSession: () => ({ user: { id: 'user-test' }, activeOrg: { id: 'org-test' } }) }))

const key = chatOrganizationStorageKey('user-test', 'org-test')!
const thread = { threadId: 'thread-a', title: 'Planning', preview: 'A draft', updatedAt: '2026-09-28T12:00:00Z' }
function mount() {
  return render(() => <ChatSidebarOrganizer sessions={[thread]} loading={false} onNewChat={() => {}} renderRow={(item, onMove) => <button type="button" onClick={() => onMove?.()}>Move {item.title}</button>} />)
}

function seedProjects() {
  saveChatOrganization(key, {
    ...emptyChatOrganization(),
    projects: [{ id: 'work', name: 'Work', pinned: false }, { id: 'later', name: 'Later', pinned: false }],
    assignments: { 'thread-a': 'work' },
  })
}
function dragTransfer() {
  const entries = new Map<string, string>()
  return {
    get types() { return [...entries.keys()] },
    effectAllowed: 'none',
    dropEffect: 'none',
    setData: (type: string, value: string) => entries.set(type, value),
    getData: (type: string) => entries.get(type) ?? '',
  }
}

beforeEach(() => {
  localStorage.clear()
  sessionStorage.clear()
  HTMLDialogElement.prototype.showModal = function () { this.setAttribute('open', '') }
  HTMLDialogElement.prototype.close = function () { this.removeAttribute('open') }
})
afterEach(cleanup)

describe('chat organizer star and unread', () => {
  const pair = [
    { threadId: 'thread-a', title: 'Planning', preview: 'A draft', updatedAt: '2026-09-28T12:00:00Z' },
    { threadId: 'thread-b', title: 'Research', preview: 'Notes', updatedAt: '2026-09-27T12:00:00Z' },
  ]
  function mountWith(extra: { starredThreadIds?: ReadonlySet<string>; unreadThreadIds?: ReadonlySet<string> }) {
    return render(() => (
      <ChatSidebarOrganizer
        sessions={pair}
        loading={false}
        onNewChat={() => {}}
        renderRow={(item) => <span>{item.title}</span>}
        {...extra}
      />
    ))
  }

  it('shows an unread dot only for a thread holding a reply the owner has not read', () => {
    mountWith({ unreadThreadIds: new Set(['thread-b']) })
    flush()
    const dots = screen.getAllByRole('img', { name: 'Unread reply' })
    expect(dots).toHaveLength(1)
    expect(dots[0]?.closest('.chat-organizer__draggable')?.textContent).toContain('Research')
  })

  it('shows no unread dot when every thread has been read', () => {
    mountWith({ unreadThreadIds: new Set() })
    flush()
    expect(screen.queryByRole('img', { name: 'Unread reply' })).toBeNull()
  })

  it('narrows the list to starred threads and clears the filter again', () => {
    mountWith({ starredThreadIds: new Set(['thread-a']) })
    flush()
    expect(screen.getByText('Research')).toBeTruthy()
    const toggle = screen.getByRole('button', { name: 'Starred' })
    expect(toggle.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(toggle)
    flush()
    expect(toggle.getAttribute('aria-pressed')).toBe('true')
    expect(screen.queryByText('Research')).toBeNull()
    expect(screen.getByText('Planning')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: /Clear filters/ }))
    flush()
    expect(screen.getByText('Research')).toBeTruthy()
  })

  it('hides the Starred filter when the caller passes no starred set', () => {
    mountWith({})
    flush()
    expect(screen.queryByRole('button', { name: 'Starred' })).toBeNull()
    expect(screen.getByText('Research')).toBeTruthy()
  })
})

describe('chat organizer interactions', () => {
  it('creates a project and switches the view atomically, preserving it after remount', async () => {
    const first = mount()
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'New project' }))
    flush()
    const input = await screen.findByRole('textbox', { name: 'Project name' })
    fireEvent.input(input, { target: { value: 'Research' } })
    flush()
    fireEvent.submit(input.closest('form')!)
    flush()
    await waitFor(() => expect(screen.getByText('Research')).toBeTruthy())
    expect(readChatOrganization(key).projects).toHaveLength(1)
    first.unmount()
    mount()
    flush()
    expect(await screen.findByText('Research')).toBeTruthy()
  })

  it('assigns a chat to a project and retains the chat when that folder is removed', async () => {
    mount()
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'New project' }))
    flush()
    const input = await screen.findByRole('textbox', { name: 'Project name' })
    fireEvent.input(input, { target: { value: 'Work' } })
    flush()
    fireEvent.submit(input.closest('form')!)
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Move Planning' }))
    flush()
    const select = await screen.findByRole('combobox', { name: /^Project$/ })
    const project = readChatOrganization(key).projects[0]!
    fireEvent.change(select, { target: { value: project.id } })
    flush()
    fireEvent.submit(select.closest('form')!)
    flush()
    expect(readChatOrganization(key).assignments['thread-a']).toBe(project.id)
    fireEvent.click(screen.getByRole('button', { name: 'Manage project Work' }))
    flush()
    fireEvent.click(await screen.findByRole('button', { name: 'Remove project, keep chats' }))
    flush()
    expect(readChatOrganization(key).projects).toEqual([])
    expect(readChatOrganization(key).assignments).toEqual({})
    expect(screen.getByRole('button', { name: 'Move Planning' })).toBeTruthy()
  })

  it('tells the person why a project could not be saved: a duplicate name is not the generic "Could not save."', async () => {
    seedProjects()
    mount()
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'New project' }))
    flush()
    const input = await screen.findByRole('textbox', { name: 'Project name' })
    fireEvent.input(input, { target: { value: ' work ' } })
    flush()
    fireEvent.submit(input.closest('form')!)
    flush()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toBe('A project with this name already exists.')
    expect(readChatOrganization(key).projects).toHaveLength(2)
  })

  it('drags a chat into a collapsed project and back to an empty Recent group', () => {
    seedProjects()
    const view = mount()
    flush()
    const later = view.container.querySelector<HTMLDetailsElement>('[data-drop-target="later"]')!
    later.open = false
    const transfer = dragTransfer()
    const source = () => screen.getByRole('button', { name: 'Move Planning' }).closest('[draggable]')!
    fireEvent.dragStart(source(), { dataTransfer: transfer })
    flush()
    expect(transfer.effectAllowed).toBe('move')
    fireEvent.dragOver(later.querySelector('summary')!, { dataTransfer: transfer })
    flush()
    expect(later.getAttribute('data-drop-active')).toBe('true')
    fireEvent.drop(later.querySelector('summary')!, { dataTransfer: transfer })
    flush()
    expect(readChatOrganization(key).assignments['thread-a']).toBe('later')
    expect(screen.getByRole('status').textContent).toBe('Moved to Later.')
    expect(view.container.querySelector('[data-dragged="true"]')).toBeNull()

    // A drop destination is retained even while every chat belongs to a project.
    const recent = view.container.querySelector('[data-drop-target="recent"]')!
    fireEvent.dragStart(source(), { dataTransfer: transfer })
    fireEvent.dragOver(recent, { dataTransfer: transfer })
    fireEvent.drop(recent, { dataTransfer: transfer })
    flush()
    expect(readChatOrganization(key).assignments).toEqual({})
    expect(screen.getByRole('status').textContent).toBe('Moved to Recent.')
    expect(screen.getByRole('button', { name: 'Move Planning' })).toBeTruthy()
  })

  it('ignores external, mismatched and cancelled drags without changing assignments', () => {
    seedProjects()
    const view = mount()
    flush()
    const target = view.container.querySelector('[data-drop-target="later"]')!
    const source = screen.getByRole('button', { name: 'Move Planning' }).closest('[draggable]')!
    const transfer = dragTransfer()
    transfer.setData('text/plain', 'An external file or selection')
    fireEvent.drop(target, { dataTransfer: transfer })
    flush()
    expect(readChatOrganization(key).assignments['thread-a']).toBe('work')

    fireEvent.dragStart(source, { dataTransfer: transfer })
    transfer.setData('application/x-verevon-chat-thread', 'another-thread')
    fireEvent.drop(target, { dataTransfer: transfer })
    flush()
    expect(readChatOrganization(key).assignments['thread-a']).toBe('work')

    fireEvent.dragStart(source, { dataTransfer: transfer })
    fireEvent.dragOver(target, { dataTransfer: transfer })
    flush()
    fireEvent.keyDown(window, { key: 'Escape' })
    flush()
    expect(view.container.querySelector('[data-drop-active="true"]')).toBeNull()
    fireEvent.drop(target, { dataTransfer: transfer })
    flush()
    expect(readChatOrganization(key).assignments['thread-a']).toBe('work')
  })

  it('preserves the Move menu when the date view does not expose project drop targets', () => {
    seedProjects()
    const view = mount()
    flush()
    fireEvent.change(screen.getByLabelText('Group by'), { target: { value: 'date' } })
    flush()
    const source = screen.getByRole('button', { name: 'Move Planning' }).closest('[draggable]')!
    expect(source.getAttribute('draggable')).toBe('false')
    expect(view.container.querySelector('[data-drop-target]')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Move Planning' }))
    flush()
    expect(screen.getByLabelText('Project')).toBeTruthy()
  })

  it('offers Load more only while the server has more chats, and asks for the next page', () => {
    const onLoadMore = vi.fn()
    render(() => <ChatSidebarOrganizer sessions={[thread]} loading={false} hasMore onLoadMore={onLoadMore} onNewChat={() => {}} renderRow={(item) => <span>{item.title}</span>} />)
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Load more chats' }))
    expect(onLoadMore).toHaveBeenCalledTimes(1)
  })

  it('hides Load more on the last page', () => {
    mount()
    flush()
    expect(screen.queryByRole('button', { name: 'Load more chats' })).toBeNull()
  })
})

describe('chat organizer server search', () => {
  const hit: ChatSearchHit = {
    threadId: 'thread-old',
    messageId: 'message-1',
    role: 'assistant',
    snippet: 'the receipt is filed',
    createdAt: '2026-08-02T09:00:00Z',
    threadTitle: 'Old receipt chat',
    archived: false,
  }
  const second: ChatSearchHit = { ...hit, threadId: 'thread-two', messageId: 'message-2', threadTitle: 'Second receipt chat' }

  function renderSidebar(onOpenSearchHit = vi.fn()) {
    render(() => (
      <ChatSidebarOrganizer
        sessions={[thread]}
        loading={false}
        onNewChat={() => {}}
        onOpenSearchHit={onOpenSearchHit}
        renderRow={(item) => <span>{item.title}</span>}
      />
    ))
    flush()
    fireEvent.click(screen.getByRole('button', { name: 'Search chats' }))
    flush()
    return onOpenSearchHit
  }
  function typeQuery(value: string) {
    const input = screen.getByRole('combobox', { name: 'Search chats' })
    fireEvent.input(input, { target: { value } })
    flush()
    return input
  }

  beforeEach(() => {
    searchChatThreads.mockReset()
  })

  it('asks the server once after the typing pauses, and shows its hits as options', async () => {
    searchChatThreads.mockResolvedValue({ status: 'ok', hits: [hit] })
    const onOpenSearchHit = renderSidebar()

    typeQuery('rec')
    typeQuery('receipt')
    expect(searchChatThreads).not.toHaveBeenCalled()

    await waitFor(() => expect(searchChatThreads).toHaveBeenCalledTimes(1))
    expect(searchChatThreads).toHaveBeenCalledWith('receipt')
    fireEvent.click(await screen.findByRole('option', { name: /Old receipt chat/ }))
    expect(onOpenSearchHit).toHaveBeenCalledWith('thread-old')
  })

  it('moves the selection with the arrow keys and opens the selected hit with Enter', async () => {
    searchChatThreads.mockResolvedValue({ status: 'ok', hits: [hit, second] })
    const onOpenSearchHit = renderSidebar()

    const input = typeQuery('receipt')
    await screen.findByRole('option', { name: /Second receipt chat/ })
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    flush()
    fireEvent.keyDown(input, { key: 'ArrowDown' })
    flush()
    expect(input.getAttribute('aria-activedescendant')).toBe('chat-organizer-search-option-1')
    expect(screen.getByRole('option', { name: /Second receipt chat/ }).getAttribute('aria-selected')).toBe('true')
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onOpenSearchHit).toHaveBeenCalledWith('thread-two')
  })

  it('keeps the local filter working and says so when search is unavailable', async () => {
    searchChatThreads.mockResolvedValue({ status: 'unavailable', reason: 'disabled' })
    renderSidebar()

    typeQuery('Planning')
    expect(await screen.findByText(/Search is unavailable/)).toBeTruthy()
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(screen.getByText('Planning')).toBeTruthy()
  })

  it('does not ask the server for a single character', async () => {
    renderSidebar()

    typeQuery('r')
    await new Promise((resolve) => setTimeout(resolve, SEARCH_DEBOUNCE_MS))
    expect(searchChatThreads).not.toHaveBeenCalled()
  })
})
