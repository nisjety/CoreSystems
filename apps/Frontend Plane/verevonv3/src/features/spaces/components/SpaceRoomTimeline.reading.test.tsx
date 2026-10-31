import { cleanup, render, screen } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// U3: the timeline reads like Slack and Teams. A time beside each author, same-author turns collapsed
// under one header, one "new messages" divider above the first new post, and a typed 403 said in the
// denial presenter's words. The client is mocked: no request is made.

const spacesClient = vi.hoisted(() => ({
  getSpaceThreadTranscript: vi.fn(),
  loadSpaceTranscript: vi.fn(),
  updateSpaceThreadPresentation: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', () => spacesClient)
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')
const { normalizeTranscriptTurns } = await import('./sidepane/SpaceTurnList')

const roster = [
  { subject_id: 'u1', display_name: 'Kari Nordmann' },
  { subject_id: 'u2', display_name: 'Ola Hansen' },
]
const agents = [{ agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active' }]

// Local components so the HH:MM expectations hold in any time zone.
const at = (hour: number, minute: number, second = 0) => new Date(2026, 9, 31, hour, minute, second).toISOString()

function renderRoom(threads: readonly unknown[], extra: Record<string, unknown> = {}) {
  return render(() => (
    <SpaceRoomTimeline
      spaceRef="room-1"
      spaceName="Lager"
      threads={() => threads as never}
      roster={() => roster as never}
      agents={() => agents as never}
      {...(extra as object)}
    />
  ))
}

function transcript(turns: readonly unknown[]) {
  spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns })
}

const post = { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'completed' }

beforeEach(() => {
  spacesClient.getSpaceThreadTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockReset()
  spacesClient.updateSpaceThreadPresentation.mockReset()
  transcript([])
})

afterEach(() => cleanup())

describe('SpaceRoomTimeline reading: per-message time', () => {
  it('shows a short time beside the author, with the full date and time on hover and in datetime', async () => {
    transcript([{ role: 'user', content: 'Hei alle', authorSubjectId: 'u1', createdAt: at(14, 32, 5) }])
    renderRoom([post])
    await screen.findByText('Hei alle')
    const time = document.querySelector('.verevon-room-turn__header time') as HTMLTimeElement
    expect(time.textContent).toBe('14:32')
    expect(time.getAttribute('datetime')).toBe(at(14, 32, 5))
    expect(time.getAttribute('title')).toMatch(/2026.*14:32/)
    // It sits in the header, beside the name.
    expect(time.parentElement?.textContent).toContain('Kari Nordmann')
  })

  it('shows no time at all when the turn has none, rather than inventing one', async () => {
    transcript([{ role: 'user', content: 'Gammel melding', authorSubjectId: 'u1' }])
    renderRoom([post])
    await screen.findByText('Gammel melding')
    expect(document.querySelector('.verevon-room-turn time')).toBeNull()
  })

  it('shows no time for a createdAt that is not a time', async () => {
    transcript([{ role: 'user', content: 'Rar tid', authorSubjectId: 'u1', createdAt: 'yesterday-ish' }])
    renderRoom([post])
    await screen.findByText('Rar tid')
    expect(document.querySelector('.verevon-room-turn time')).toBeNull()
  })
})

describe('SpaceRoomTimeline reading: same-author grouping', () => {
  it('collapses consecutive turns by one member within five minutes under one name', async () => {
    transcript([
      { role: 'user', content: 'Første', authorSubjectId: 'u1', createdAt: at(10, 0) },
      { role: 'user', content: 'Andre', authorSubjectId: 'u1', createdAt: at(10, 3) },
      { role: 'user', content: 'Tredje', authorSubjectId: 'u1', createdAt: at(10, 7) },
    ])
    renderRoom([post])
    await screen.findByText('Tredje')
    // 'Tredje' is 4 minutes after 'Andre' (the window runs from the turn above, not the group's start).
    expect(document.querySelectorAll('.verevon-room-turn__author').length).toBe(1)
    expect(document.querySelectorAll('.verevon-room-turn[data-grouped]').length).toBe(2)
    expect(document.querySelectorAll('.verevon-room-turn__avatar').length).toBe(1)
  })

  it('starts a new header after a gap longer than five minutes', async () => {
    transcript([
      { role: 'user', content: 'Morgen', authorSubjectId: 'u1', createdAt: at(10, 0) },
      { role: 'user', content: 'Senere', authorSubjectId: 'u1', createdAt: at(10, 6) },
    ])
    renderRoom([post])
    await screen.findByText('Senere')
    expect(document.querySelectorAll('.verevon-room-turn__author').length).toBe(2)
    expect(document.querySelector('.verevon-room-turn[data-grouped]')).toBeNull()
  })

  it('keeps a grouped turn’s own time in its gutter and out of the header', async () => {
    transcript([
      { role: 'user', content: 'Første', authorSubjectId: 'u1', createdAt: at(10, 0) },
      { role: 'user', content: 'Andre', authorSubjectId: 'u1', createdAt: at(10, 2) },
    ])
    renderRoom([post])
    await screen.findByText('Andre')
    const grouped = document.querySelector('.verevon-room-turn[data-grouped]') as HTMLElement
    expect(grouped.querySelector('.verevon-room-turn__header')).toBeNull()
    expect(grouped.querySelector('.verevon-room-turn__gutter time')?.textContent).toBe('10:02')
  })

  it('never groups a member with an agent, or two different members', async () => {
    transcript([
      { role: 'user', content: 'Spørsmål', authorSubjectId: 'u1', createdAt: at(10, 0) },
      { role: 'assistant', content: 'Svar', agentName: 'Lagerhjelpen', createdAt: at(10, 1) },
      { role: 'user', content: 'Takk', authorSubjectId: 'u2', createdAt: at(10, 2) },
      { role: 'assistant', content: 'Bare hyggelig', agentName: 'Lagerhjelpen', createdAt: at(10, 3) },
    ])
    renderRoom([post])
    await screen.findByText('Bare hyggelig')
    expect(document.querySelectorAll('.verevon-room-turn__author').length).toBe(4)
    expect(document.querySelector('.verevon-room-turn[data-grouped]')).toBeNull()
  })

  it('groups two answers by the same agent', async () => {
    transcript([
      { role: 'assistant', content: 'Del en', agentName: 'Lagerhjelpen', createdAt: at(10, 0) },
      { role: 'assistant', content: 'Del to', agentName: 'Lagerhjelpen', createdAt: at(10, 1) },
    ])
    renderRoom([post])
    await screen.findByText('Del to')
    expect(document.querySelectorAll('.verevon-room-turn__author').length).toBe(1)
    expect(document.querySelectorAll('.verevon-room-turn[data-grouped]').length).toBe(1)
  })

  it('does not group turns without times, so older rows read exactly as before', async () => {
    transcript([
      { role: 'user', content: 'En', authorSubjectId: 'u1' },
      { role: 'user', content: 'To', authorSubjectId: 'u1' },
    ])
    renderRoom([post])
    await screen.findByText('To')
    expect(document.querySelectorAll('.verevon-room-turn__author').length).toBe(2)
  })
})

describe('SpaceRoomTimeline reading: the new messages divider', () => {
  const older = { thread_id: 'old', space_id: 'room-1', title: 'Gammel', updated_at: '2026-10-31T08:00:00Z', latest_run_status: 'completed' }
  const fresh1 = { thread_id: 'n1', space_id: 'room-1', title: 'Ny en', updated_at: '2026-10-31T09:00:00Z', latest_run_status: 'completed' }
  const fresh2 = { thread_id: 'n2', space_id: 'room-1', title: 'Ny to', updated_at: '2026-10-31T10:00:00Z', latest_run_status: 'completed' }

  it('draws one divider, above the first new post, and not above the posts after it', async () => {
    renderRoom([fresh2, older, fresh1], { unreadThreadIds: () => new Set(['n1', 'n2']) })
    await screen.findByText('Ny to')
    const dividers = document.querySelectorAll('[role="separator"]')
    expect(dividers.length).toBe(1)
    expect(dividers[0]?.getAttribute('aria-label')).toBe('Nye meldinger')
    // Order in the list: old post, divider, first new post, second new post.
    const items = [...document.querySelectorAll('.verevon-room-timeline > li')]
    const kinds = items.map((item) =>
      item.classList.contains('verevon-room-new-divider-item') ? 'divider' : item.textContent?.includes('Gammel') ? 'old' : item.textContent?.includes('Ny en') ? 'n1' : 'n2',
    )
    expect(kinds).toEqual(['old', 'divider', 'n1', 'n2'])
  })

  it('keeps an accessible label on each new post but marks the divider as the visual', async () => {
    renderRoom([older, fresh1], { unreadThreadIds: () => new Set(['n1']) })
    await screen.findByText('Ny en')
    const flag = document.querySelector('.verevon-room-post[data-unread] .verevon-room-post__flag--new')
    expect(flag?.textContent).toBe('Ny')
  })

  it('draws no divider when nothing is new, or when the page gave no derivation', async () => {
    renderRoom([older, fresh1], { unreadThreadIds: () => new Set<string>() })
    await screen.findByText('Ny en')
    expect(document.querySelector('[role="separator"]')).toBeNull()
    cleanup()
    renderRoom([older, fresh1])
    await screen.findByText('Ny en')
    expect(document.querySelector('[role="separator"]')).toBeNull()
  })
})

describe('SpaceRoomTimeline reading: a typed 403 is said in the denial presenter’s words', () => {
  const typed = {
    code: 'space_thread_not_authorized',
    details: { reason: 'space_role_below_floor', resolvable_by: 'space_manager' },
  }

  it('shows the presenter’s copy for the named reason instead of the generic shared-read notice', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'restricted', denial: typed })
    renderRoom([{ ...post, preview: 'Forhåndsvisning' }])
    expect(await screen.findByText(/Rollen din i dette rommet er ikke høy nok til dette/)).toBeTruthy()
    expect(screen.queryByText(/Denne samtalen er ikke delt med deg/)).toBeNull()
    expect(screen.getByText('Forhåndsvisning')).toBeTruthy()
  })

  it('uses the presenter’s generic line for a reason this build does not know', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({
      state: 'restricted',
      denial: { code: 'forbidden', details: { reason: 'a_newer_reason' } },
    })
    renderRoom([post])
    expect(await screen.findByText('Dette er ikke tillatt i rommet akkurat nå.')).toBeTruthy()
  })

  it('keeps the shared-read notice for a 403 that carries no typed reason', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'restricted', denial: { code: 'forbidden' } })
    renderRoom([post])
    expect(await screen.findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
  })
})

describe('normalizeTranscriptTurns: createdAt', () => {
  it('keeps a readable time as an ISO string and drops anything else', () => {
    const turns = normalizeTranscriptTurns([
      { role: 'user', content: 'a', createdAt: '2026-10-31T09:30:15.250Z' },
      { role: 'user', content: 'b', createdAt: 'nonsense' },
      { role: 'user', content: 'c', createdAt: 42 },
      { role: 'user', content: 'd' },
    ])
    expect(turns.map((turn) => turn.createdAt)).toEqual(['2026-10-31T09:30:15.250Z', undefined, undefined, undefined])
    expect('createdAt' in (turns[1] as object)).toBe(false)
  })
})

// W1 close: the date and the status sit beside the author, not in a footer line under the post, and a
// person has one colour wherever the room draws them.
describe('SpaceRoomTimeline reading: date and status beside the author', () => {
  const dated = { ...post, updated_at: '2026-10-27T10:00:00Z' }

  it('shows the post date in the header of the first row when its turns carry no time', async () => {
    transcript([{ role: 'user', content: 'Gammel melding', authorSubjectId: 'u1' }])
    renderRoom([dated])
    await screen.findByText('Gammel melding')
    const header = document.querySelector('.verevon-room-turn__header') as HTMLElement
    expect(header.querySelector('.verevon-room-turn__time')?.textContent).toMatch(/27\. okt/)
    // A date is not a time of day: it is not marked up as one.
    expect(header.querySelector('time')).toBeNull()
    // No orphan date under the post.
    expect(document.querySelector('.verevon-room-post__meta time')).toBeNull()
  })

  it('shows the time of the turn, not the date, when the turn has one', async () => {
    transcript([{ role: 'user', content: 'Ny melding', authorSubjectId: 'u1', createdAt: at(14, 32) }])
    renderRoom([dated])
    await screen.findByText('Ny melding')
    const header = document.querySelector('.verevon-room-turn__header') as HTMLElement
    expect(header.querySelector('time')?.textContent).toBe('14:32')
    expect(header.textContent).not.toMatch(/okt/)
  })

  it('says a failed run beside the author of the first row, coloured as a failure', async () => {
    transcript([{ role: 'user', content: 'Mislyktes', authorSubjectId: 'u1' }])
    renderRoom([{ ...dated, latest_run_status: 'failed' }])
    await screen.findByText('Mislyktes')
    const status = document.querySelector('.verevon-room-turn__header .verevon-space-status--failed')
    expect(status?.textContent).toBe('Feilet')
    expect(document.querySelector('.verevon-room-post__meta .verevon-space-status')).toBeNull()
  })

  it('says a post with no transcript yet beside its starter, status included', async () => {
    renderRoom([{ ...dated, owner_subject_id: 'u1', preview: 'Forhåndsvisning', latest_run_status: 'cancelled' }])
    await screen.findByText('Forhåndsvisning')
    const header = document.querySelector('.verevon-room-turn__header') as HTMLElement
    expect(header.textContent).toContain('Kari Nordmann')
    expect(header.textContent).toContain('Stoppet')
    expect(header.querySelector('.verevon-room-turn__time')?.textContent).toMatch(/27\. okt/)
  })

  it('says a working run on the agent row, not on the first row', async () => {
    transcript([{ role: 'user', content: 'Gjør det', authorSubjectId: 'u1' }])
    renderRoom([{ ...dated, latest_run_status: 'running' }])
    await screen.findByText('Gjør det')
    const active = document.querySelectorAll('.verevon-space-status--active')
    expect(active.length).toBe(1)
    expect(active[0]?.closest('.verevon-room-turn--working')).toBeTruthy()
  })

  it('says nothing visible for a finished post, but keeps the label for assistive tech', async () => {
    transcript([{ role: 'user', content: 'Ferdig', authorSubjectId: 'u1' }])
    renderRoom([dated])
    await screen.findByText('Ferdig')
    expect(document.querySelector('.verevon-room-turn__header .verevon-space-status')).toBeNull()
    expect(document.querySelector('.verevon-room-post__status-quiet')?.textContent).toBe('Fullført')
  })
})

describe('SpaceRoomTimeline reading: one colour per person', () => {
  it('draws a member in their own tint (the header stack tint), and an unattributed turn neutral', async () => {
    const { memberTint } = await import('../lib/agent-avatar')
    transcript([
      { role: 'user', content: 'Fra Kari', authorSubjectId: 'u1', createdAt: at(10, 0) },
      { role: 'user', content: 'Fra Ola', authorSubjectId: 'u2', createdAt: at(11, 0) },
      { role: 'user', content: 'Fra ingen', createdAt: at(12, 0) },
    ])
    renderRoom([post])
    await screen.findByText('Fra ingen')
    const avatars = Array.from(document.querySelectorAll<HTMLElement>('.verevon-room-turn__avatar'))
    expect(avatars.map((avatar) => avatar.style.getPropertyValue('--avatar-hue'))).toEqual([
      memberTint('u1'),
      memberTint('u2'),
      '',
    ])
  })
})
