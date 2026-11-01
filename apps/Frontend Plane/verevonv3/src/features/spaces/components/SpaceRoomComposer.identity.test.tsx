import { cleanup, fireEvent, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS07-S7: the @-mention picker row paints an AgentAvatar. Own file so the composer's send-path spec
// stays about sending.

const chatClient = vi.hoisted(() => ({
  streamChat: vi.fn(),
  cancelInvocation: vi.fn(),
  listModels: vi.fn(),
}))
vi.mock('@/shared/api/chat-client', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/shared/api/chat-client')>(),
  ...chatClient,
}))
const subscriptionsClient = vi.hoisted(() => ({ listChatGptSubscriptions: vi.fn() }))
vi.mock('@/shared/api/chatgpt-subscription-client', async (importOriginal) => ({
  ...await importOriginal<typeof import('@/shared/api/chatgpt-subscription-client')>(),
  ...subscriptionsClient,
}))
vi.mock('@/shared/session/session-store', () => ({
  getSession: () => ({ activeOrg: { id: 'org-1', name: 'Test', role: 'admin' } }),
}))
const spacesClient = vi.hoisted(() => ({
  updateSpaceThreadPresentation: vi.fn(),
  createSpacePost: vi.fn(),
  replyInSpaceThread: vi.fn(),
  SPACE_POST_MAX_CHARS_DEFAULT: 8000,
}))
vi.mock('@/shared/api/spaces-client', () => spacesClient)
const skillsClient = vi.hoisted(() => ({ listAvailableSkills: vi.fn(), MAX_PICKED_SKILLS: 4 }))
vi.mock('@/shared/api/skills-client', () => skillsClient)
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))

const { SpaceRoomComposer } = await import('./SpaceRoomComposer')

const BLUE = 'rgb(37, 99, 235)'
const RED = 'rgb(220, 38, 38)'

const agentOf = (overrides: Record<string, unknown>) => ({
  subject_id: 'agent-1',
  role: 'editor',
  revision: 1,
  identity_published: true,
  name: 'Lagerhjelpen',
  status: 'active',
  delivery_targets: [],
  ...overrides,
})

const renderComposer = (agents: readonly unknown[]) =>
  render(() => (
    <SpaceRoomComposer
      spaceRef="room-1"
      roster={() => [
        { subject_type: 'user', subject_id: 'user-2', role: 'editor', revision: 1, display_name: 'Lars Hansen' },
      ] as never}
      agents={() => agents as never}
      threads={() => []}
    />
  ))

function openPicker() {
  fireEvent.input(screen.getByRole('textbox'), { target: { value: '@' } })
  flush()
}

beforeEach(() => {
  // DEPTH-SPA-2: the connectivity banner writes a signal while mounting; it has its own switch.
  vi.stubEnv('VITE_SPACE_CONNECTIVITY_BANNER_UI', 'false')
  chatClient.streamChat.mockReset()
  chatClient.listModels.mockReset()
  chatClient.listModels.mockResolvedValue([])
  subscriptionsClient.listChatGptSubscriptions.mockReset()
  subscriptionsClient.listChatGptSubscriptions.mockResolvedValue([])
  skillsClient.listAvailableSkills.mockReset()
  skillsClient.listAvailableSkills.mockResolvedValue([])
})

afterEach(() => {
  vi.unstubAllEnvs()
  cleanup()
  window.localStorage.clear()
})

describe('SpaceRoomComposer mention picker avatars (WS07-S7)', () => {
  it('an agent row renders an AgentAvatar in the agent colour, with the name beside it', async () => {
    renderComposer([agentOf({ avatar_color: '#2563eb' })])
    openPicker()

    const row = await screen.findByRole('option', { name: /Lagerhjelpen/ })
    const avatar = row.querySelector('.verevon-agent-avatar') as HTMLElement
    expect(avatar).toBeTruthy()
    expect(avatar.getAttribute('data-kind')).toBe('agent')
    expect(avatar.style.background).toBe(BLUE)
    expect(row.textContent).toContain('Lagerhjelpen')
  })

  it('each row takes its own agent colour, even for two agents with the same name', async () => {
    renderComposer([
      agentOf({ subject_id: 'agent-1', avatar_color: '#2563eb' }),
      agentOf({ subject_id: 'agent-2', avatar_color: '#dc2626' }),
    ])
    openPicker()

    const rows = await screen.findAllByRole('option', { name: /Lagerhjelpen/ })
    expect(rows).toHaveLength(2)
    const colours = rows.map((row) => (row.querySelector('.verevon-agent-avatar') as HTMLElement).style.background)
    expect(colours.sort()).toEqual([BLUE, RED].sort())
  })

  it('an agent with no colour or an invalid one renders a neutral avatar', async () => {
    renderComposer([
      agentOf({ subject_id: 'agent-1', name: 'Uten farge' }),
      agentOf({ subject_id: 'agent-2', name: 'Feil farge', avatar_color: 'url(x)' }),
    ])
    openPicker()

    for (const name of [/Uten farge/, /Feil farge/]) {
      const avatar = (await screen.findByRole('option', { name })).querySelector('.verevon-agent-avatar') as HTMLElement
      expect(avatar.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
      expect(avatar.style.background).toBe('')
    }
  })

  it('a person row has no agent avatar, and an unpublished or paused agent is not offered', async () => {
    renderComposer([
      agentOf({ subject_id: 'agent-1', name: 'Lagerhjelpen', avatar_color: '#2563eb' }),
      agentOf({ subject_id: 'agent-2', name: 'Pauset', status: 'paused', avatar_color: '#dc2626' }),
      agentOf({ subject_id: 'agent-3', name: 'Upublisert', identity_published: false, avatar_color: '#dc2626' }),
    ])
    openPicker()

    const person = await screen.findByRole('option', { name: /Lars Hansen/ })
    expect(person.querySelector('.verevon-agent-avatar')).toBeNull()
    expect(screen.queryByRole('option', { name: /Pauset/ })).toBeNull()
    expect(screen.queryByRole('option', { name: /Upublisert/ })).toBeNull()
  })
})
