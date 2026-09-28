import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ChatSidebarOrganizer } from './ChatSidebarOrganizer'
import { chatOrganizationStorageKey, emptyChatOrganization, readChatOrganization, saveChatOrganization } from '@/features/chat/lib/chat-organization'

vi.mock('@/shared/i18n', () => ({ useI18n: () => ({ tr: (_no: string, en: string) => en }) }))
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
})
