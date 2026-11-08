import { cleanup, render, screen, within } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS07-S7 (avatars) and WS07-S8 (identity hover card) on the room timeline and its turn list. Kept in
// its own file so the timeline's main spec stays about the room's behaviour.

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  getSpaceThreadTranscript: vi.fn(),
  loadSpaceTranscript: vi.fn(),
  updateSpaceThreadPresentation: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')
const { SpaceTurnList } = await import('./sidepane/SpaceTurnList')

const BLUE = 'rgb(37, 99, 235)'

const lager = {
  subject_id: 'agent-1',
  role: 'editor',
  revision: 1,
  identity_published: true,
  name: 'Lagerhjelpen',
  status: 'active',
  avatar_color: '#2563eb',
  delivery_targets: [],
}
const innkjop = { ...lager, subject_id: 'agent-2', name: 'Innkjøpsagent', avatar_color: '#dc2626' }
const roster = [
  { subject_type: 'user', subject_id: 'user-1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
  { subject_type: 'user', subject_id: 'user-2', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
]
const post = {
  thread_id: 't1',
  space_id: 'room-1',
  title: 'Innkjøp',
  owner_subject_id: 'user-2',
  updated_at: '2026-10-02T10:00:00Z',
  latest_run_status: 'completed',
}

const renderRoom = (agents: readonly unknown[], threads: readonly unknown[] = [post]) =>
  render(() => (
    <SpaceRoomTimeline
      spaceRef="room-1"
      spaceName="Lager"
      threads={() => threads as never}
      roster={() => roster as never}
      agents={() => agents as never}
    />
  ))

const withTurns = (turns: readonly unknown[]) =>
  spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns })

const assistantTurn = (agentName: string) => ({ role: 'assistant', content: 'Alt er klart.', agentName })

/** The avatar and the author wrapper of the one turn whose content is `text`. */
function turnRow(text: string) {
  const row = screen.getByText(text).closest('.verevon-room-turn') as HTMLElement
  return {
    row,
    avatar: row.querySelector('.verevon-agent-avatar') as HTMLElement | null,
    trigger: row.querySelector('.verevon-room-turn__author .verevon-identity-trigger') as HTMLElement | null,
    author: row.querySelector('.verevon-room-turn__author') as HTMLElement,
  }
}

beforeEach(() => {
  spacesClient.getSpaceThreadTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns: [] })
  spacesClient.updateSpaceThreadPresentation.mockReset()
})

afterEach(() => cleanup())

describe('SpaceRoomTimeline avatars (WS07-S7)', () => {
  it('an assistant turn naming exactly one roster agent renders its colour', async () => {
    withTurns([assistantTurn('Lagerhjelpen')])
    renderRoom([lager, innkjop])
    await screen.findByText('Alt er klart.')

    const { avatar } = turnRow('Alt er klart.')
    expect(avatar?.getAttribute('data-kind')).toBe('agent')
    expect(avatar?.getAttribute('aria-hidden')).toBe('true')
    expect(avatar?.style.background).toBe(BLUE)
    expect(avatar?.classList.contains('verevon-agent-avatar--neutral')).toBe(false)
    // Colour is never the only signal: the name and the Agent chip stay.
    expect(within(turnRow('Alt er klart.').row).getByText('Agent')).toBeTruthy()
  })

  it('same-named agents render neutral rather than guessing a colour', async () => {
    withTurns([assistantTurn('Lagerhjelpen')])
    renderRoom([lager, { ...lager, subject_id: 'agent-3', avatar_color: '#16a34a' }])
    await screen.findByText('Alt er klart.')

    const { avatar, trigger } = turnRow('Alt er klart.')
    expect(avatar?.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
    expect(avatar?.style.background).toBe('')
    // No card either: the identity cannot be pinned down.
    expect(trigger).toBeNull()
  })

  it('a turn naming nobody on the roster renders neutral', async () => {
    withTurns([assistantTurn('Ukjent')])
    renderRoom([lager])
    await screen.findByText('Alt er klart.')
    expect(turnRow('Alt er klart.').avatar?.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
  })

  it('an agent whose stored colour is invalid renders neutral', async () => {
    withTurns([assistantTurn('Lagerhjelpen')])
    renderRoom([{ ...lager, avatar_color: 'url(x)' }])
    await screen.findByText('Alt er klart.')
    const { avatar } = turnRow('Alt er klart.')
    expect(avatar?.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
    expect(avatar?.style.background).toBe('')
  })

  it('the live working row paints the working agent with its colour', async () => {
    renderRoom([lager], [{ ...post, latest_run_status: 'running' }])
    const row = (await screen.findByRole('status')) as HTMLElement
    expect(row.querySelector('.verevon-agent-avatar')?.getAttribute('style')).toContain('background')
    expect((row.querySelector('.verevon-agent-avatar') as HTMLElement).style.background).toBe(BLUE)
  })
})

describe('SpaceRoomTimeline identity hover cards (WS07-S8)', () => {
  it('the post author name and the agent name are each wrapped by the hover card', async () => {
    withTurns([{ role: 'user', content: 'Hva er status?', authorSubjectId: 'user-2' }, assistantTurn('Lagerhjelpen')])
    renderRoom([lager, innkjop])
    await screen.findByText('Alt er klart.')

    const human = turnRow('Hva er status?')
    expect(human.trigger?.textContent).toBe('Ola Hansen')
    expect(human.trigger?.getAttribute('tabindex')).toBe('0')

    const agentRow = turnRow('Alt er klart.')
    expect(agentRow.trigger?.textContent).toBe('Lagerhjelpen')
  })

  it('before any turn has loaded, the starter name is wrapped too', async () => {
    spacesClient.loadSpaceTranscript.mockReturnValue(new Promise(() => {}))
    renderRoom([lager])
    const author = document.querySelector('.verevon-room-turn__author') as HTMLElement
    expect(author.querySelector('.verevon-identity-trigger')?.textContent).toBe('Ola Hansen')
  })

  it('a turn without a recorded author has no card: an unnamed turn is not given an identity', async () => {
    withTurns([{ role: 'user', content: 'Gammel melding' }])
    renderRoom([lager])
    await screen.findByText('Gammel melding')
    expect(turnRow('Gammel melding').trigger).toBeNull()
  })

  it('a post with no owner has no starter card', () => {
    spacesClient.loadSpaceTranscript.mockReturnValue(new Promise(() => {}))
    renderRoom([lager], [{ ...post, owner_subject_id: undefined }])
    expect(document.querySelector('.verevon-room-turn__author .verevon-identity-trigger')).toBeNull()
  })
})

describe('SpaceTurnList in the side pane (WS07-S8)', () => {
  const turns = [
    { role: 'user' as const, content: 'Hva er status?', authorSubjectId: 'user-2' },
    { role: 'assistant' as const, content: 'Alt er klart.', agentName: 'Lagerhjelpen' },
  ]

  it('without a threads prop the names stay plain text and the avatars stay neutral', () => {
    render(() => (
      <SpaceTurnList turns={() => turns} roster={() => roster as never} agentName="Verevon" />
    ))
    expect(document.querySelector('.verevon-identity-trigger')).toBeNull()
    const agentRow = turnRow('Alt er klart.')
    expect(agentRow.author.textContent).toBe('Lagerhjelpen')
    expect(agentRow.avatar?.classList.contains('verevon-agent-avatar--neutral')).toBe(true)
    expect(turnRow('Hva er status?').author.textContent).toBe('Ola Hansen')
  })

  it('with threads and agents the same names are wrapped and the agent coloured', () => {
    render(() => (
      <SpaceTurnList
        turns={() => turns}
        roster={() => roster as never}
        agentName="Verevon"
        agents={() => [lager] as never}
        threads={() => [post] as never}
        threadId="t1"
      />
    ))
    expect(turnRow('Alt er klart.').trigger?.textContent).toBe('Lagerhjelpen')
    expect(turnRow('Alt er klart.').avatar?.style.background).toBe(BLUE)
    expect(turnRow('Hva er status?').trigger?.textContent).toBe('Ola Hansen')
  })

  it("the viewer's own avatar carries the name's initials, not the '(deg)' label's", () => {
    // The viewer's turns are labelled "Kari Nordmann (deg)"; the avatar once read "K(".
    render(() => (
      <SpaceTurnList
        turns={() => [{ role: 'user' as const, content: 'Min melding', authorSubjectId: 'user-1' }]}
        roster={() => roster as never}
        viewerSubjectId={() => 'user-1'}
        agentName="Verevon"
      />
    ))
    const row = turnRow('Min melding')
    expect(row.author.textContent).toContain('Kari Nordmann')
    expect(row.row.querySelector('.verevon-room-turn__avatar')?.textContent).toBe('KN')
  })
})
