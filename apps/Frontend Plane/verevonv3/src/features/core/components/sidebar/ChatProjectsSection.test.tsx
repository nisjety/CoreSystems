// @vitest-environment jsdom

import { createRouter, memoryHistory } from '@solidjs/router'
import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatProjectsSection } from './ChatProjectsSection'
import { I18nProvider } from '@/shared/i18n'
import { projectThreadRef, takeChatProjectStart } from '@/features/chat/lib/chat-projects'
import { readActiveChatThreadId } from '@/features/chat/lib/chat-thread-history'

const spacesClient = vi.hoisted(() => ({
  createRoom: vi.fn(),
  getSpaceContext: vi.fn(),
  getSpaceMembership: vi.fn(),
  getSpaceRoster: vi.fn(),
  getSpaceThreads: vi.fn(),
  listSpaces: vi.fn(),
}))

vi.mock('@/shared/api/spaces-client', () => ({
  createRoom: spacesClient.createRoom,
  getSpaceContext: spacesClient.getSpaceContext,
  getSpaceMembership: spacesClient.getSpaceMembership,
  getSpaceRoster: spacesClient.getSpaceRoster,
  getSpaceThreads: spacesClient.getSpaceThreads,
  listSpaces: spacesClient.listSpaces,
}))

const personalSpace = { space_ref: 'space_personal', name: 'Personal Space', kind: 'personal', lifecycle: 'active' }
const launchRoom = { space_ref: 'space_launch', name: 'Launch', kind: 'room', lifecycle: 'active', is_organization_room: false }
const orgRoom = { space_ref: 'space_org', name: 'AQUATIQ AS', kind: 'room', lifecycle: 'active', is_organization_room: true }
const readOnlyRoom = { space_ref: 'space_read', name: 'Read only', kind: 'room', lifecycle: 'active', is_organization_room: false }

function membership(spaceRef: string, role: string) {
  return { space_ref: spaceRef, org_id: 'org_1', subject_id: 'user_1', kind: 'shared', role, revisions: {} }
}

function renderSection() {
  const TestRouter = createRouter({
    routes: [
      { path: '/spaces', component: () => <ChatProjectsSection /> },
      { path: '/chat', component: () => <p>Chat page</p> },
    ],
    history: memoryHistory('/spaces'),
    explicitLinks: true,
  })
  return render(() => (
    <I18nProvider>
      <TestRouter>{(props) => <>{props.children}</>}</TestRouter>
    </I18nProvider>
  ))
}

describe('ChatProjectsSection', () => {
  beforeEach(() => {
    window.localStorage.clear()
    window.sessionStorage.clear()
    spacesClient.listSpaces.mockReset().mockResolvedValue([personalSpace, orgRoom, readOnlyRoom, launchRoom])
    spacesClient.getSpaceMembership.mockReset().mockImplementation(async (spaceRef: string) => (
      membership(spaceRef, spaceRef === 'space_read' ? 'viewer' : spaceRef === 'space_personal' ? 'owner' : 'editor')
    ))
    spacesClient.getSpaceThreads.mockReset().mockResolvedValue({
      space: launchRoom,
      membership: membership('space_launch', 'editor'),
      threads: [{ thread_id: 'thread_launch_1', space_id: 'space_launch', title: 'Launch plan', preview: 'Prepare the release.' }],
    })
    spacesClient.createRoom.mockReset()
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllEnvs()
    window.localStorage.clear()
    window.sessionStorage.clear()
  })

  it('stays out of the sidebar, and reads nothing, while the project switch is off', () => {
    renderSection()

    expect(screen.queryByText(/^(Prosjekter|Projects)$/)).toBeNull()
    expect(spacesClient.listSpaces).not.toHaveBeenCalled()
  })

  it('lists the projects the caller can chat in and leaves out the organization channel and read-only rooms', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    renderSection()

    expect(await screen.findByText('Launch')).toBeTruthy()
    expect(screen.queryByText('AQUATIQ AS')).toBeNull()
    expect(screen.queryByText('Read only')).toBeNull()
  })

  it('starts a new project chat by handing the project to the chat page and opening it', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    renderSection()

    fireEvent.click(await screen.findByRole('button', { name: /^(Ny samtale i|New chat in) Launch$/ }))

    expect(await screen.findByText('Chat page')).toBeTruthy()
    expect(takeChatProjectStart()).toBe('space_launch')
    expect(readActiveChatThreadId()).toBeNull()
  })

  it('refuses to open a project chat when neither the chat page nor tab storage can receive it', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    const refuse = vi.spyOn(window.sessionStorage, 'setItem').mockImplementation(() => {
      throw new DOMException('blocked', 'SecurityError')
    })
    renderSection()

    fireEvent.click(await screen.findByRole('button', { name: /^(Ny samtale i|New chat in) Launch$/ }))

    expect(await screen.findByText(/kunne ikke åpnes|could not be opened/)).toBeTruthy()
    expect(screen.queryByText('Chat page')).toBeNull()
    refuse.mockRestore()
  })

  it('opens a project thread by remembering its project and selecting the thread', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    renderSection()

    fireEvent.click(await screen.findByRole('button', { name: /^Launch/ }))
    fireEvent.click(await screen.findByRole('button', { name: 'Launch plan' }))

    expect(await screen.findByText('Chat page')).toBeTruthy()
    expect(spacesClient.getSpaceThreads).toHaveBeenCalledWith('space_launch')
    expect(readActiveChatThreadId()).toBe('thread_launch_1')
    expect(projectThreadRef('thread_launch_1')).toBe('space_launch')
  })

  it('creates a named room with the trimmed name and says it may take a moment to appear', async () => {
    vi.stubEnv('VITE_CHAT_PROJECTS_ENABLED', 'true')
    spacesClient.createRoom.mockResolvedValue(launchRoom)
    renderSection()

    fireEvent.click(await screen.findByRole('button', { name: /^(Nytt prosjekt|New project)$/ }))
    const input = await screen.findByRole('textbox')
    fireEvent.input(input, { target: { value: '  Q3 plan  ' } })
    flush()
    fireEvent.submit(input.closest('form') as HTMLFormElement)

    expect(await screen.findByText(/opprettet|was created/)).toBeTruthy()
    expect(spacesClient.createRoom).toHaveBeenCalledWith('Q3 plan')
  })
})
