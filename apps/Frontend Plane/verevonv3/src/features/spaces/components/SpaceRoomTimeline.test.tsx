import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const spacesClient = vi.hoisted(() => ({
  getSpaceThreadTranscript: vi.fn(),
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

const agent = { agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active' }

const renderRoom = (
  threads: readonly unknown[],
  agents: readonly unknown[] = [agent],
  extra: Record<string, unknown> = {},
) =>
  render(() => (
    <SpaceRoomTimeline
      spaceRef="room-1"
      spaceName="Lager"
      threads={() => threads as never}
      roster={() => []}
      agents={() => agents as never}
      {...(extra as object)}
    />
  ))

beforeEach(() => {
  spacesClient.getSpaceThreadTranscript.mockReset()
  spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [] })
  spacesClient.updateSpaceThreadPresentation.mockReset()
  spacesClient.updateSpaceThreadPresentation.mockResolvedValue({ thread_id: 't1' })
})

afterEach(() => cleanup())

describe('SpaceRoomTimeline — the room is visibly working', () => {
  // Item 1b. The sender had a live exchange in their composer; every OTHER
  // member saw nothing until the poll delivered a finished reply. This row is
  // what the rest of the room sees while a turn streams.
  it('shows a live working row, named for the room’s agent, while a run is running', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'running' }])

    const row = await screen.findByRole('status')
    expect(row.textContent).toContain('Lagerhjelpen')
    expect(row.textContent).toContain('jobber')
    expect(row.getAttribute('aria-live')).toBe('polite')
  })

  it('says a queued run is waiting to start rather than working', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'queued' }])
    expect(await screen.findByText(/venter på å starte/)).toBeTruthy()
  })

  // Paused on a person is active but nobody is working. Rendering it as
  // "working" would make a blocked room look busy — the opposite of the truth.
  it('does not call a run paused for approval "working"', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', latest_run_id: 'r1', latest_run_status: 'awaiting_approval' }])
    expect(await screen.findByTestId('approval-panel')).toBeTruthy()
    expect(screen.queryByText(/jobber/)).toBeNull()
  })

  it('falls back to Verevon when the room has no single active agent', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'running' }], [])
    const row = await screen.findByRole('status')
    expect(row.textContent).toContain('Verevon')
  })

  // The sheet defined this class when the cockpit shipped and nothing ever
  // applied it — a working post read in the same grey as a finished one.
  it('colours the status pill while working, and only then', async () => {
    renderRoom([
      { thread_id: 'live', space_id: 'room-1', latest_run_status: 'running' },
      { thread_id: 'done', space_id: 'room-1', latest_run_status: 'completed' },
    ])
    await screen.findByRole('status')
    const active = document.querySelectorAll('.verevon-space-status--active')
    expect(active.length).toBe(1)
    expect(active[0]?.textContent).toBe('Arbeider')
  })

  // A recorded stop. Session Core writes `cancelled` when a member presses
  // Stop; until now the room rendered that as the untranslated token
  // "Cancelled" — a stop the room itself caused, in a foreign word.
  it('renders a recorded stop as "Stoppet", not as an untranslated token', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', latest_run_status: 'cancelled' }])
    expect(await screen.findByText('Stoppet')).toBeTruthy()
    expect(screen.queryByText('Cancelled')).toBeNull()
    expect(screen.queryByText(/jobber/)).toBeNull()
  })
})

describe('SpaceRoomTimeline — room hygiene (item 4b)', () => {
  it('states a pinned post as pinned before its content', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', title: 'Rutiner', pinned: true, latest_run_status: 'completed' }])
    expect(await screen.findByText('Festet')).toBeTruthy()
    expect(document.querySelector('.verevon-room-post[data-pinned]')).toBeTruthy()
  })

  // "New" comes from the page's arrival-time derivation, never from the post's
  // own guess — the timeline only renders what it is handed.
  it('badges only the posts the page says are new since arrival', async () => {
    renderRoom(
      [
        { thread_id: 'new', space_id: 'room-1', title: 'Ny sak', latest_run_status: 'completed' },
        { thread_id: 'seen', space_id: 'room-1', title: 'Gammel sak', latest_run_status: 'completed' },
      ],
      [agent],
      { unreadThreadIds: () => new Set(['new']) },
    )
    await screen.findByText('Ny sak')
    const badges = [...document.querySelectorAll('.verevon-room-post__flag--new')]
    expect(badges.length).toBe(1)
    expect(badges[0]?.closest('.verevon-room-post')?.textContent).toContain('Ny sak')
  })

  it('badges nothing when it is handed no derivation', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', title: 'Sak', latest_run_status: 'completed' }])
    await screen.findByText('Sak')
    expect(document.querySelector('.verevon-room-post__flag--new')).toBeNull()
  })

  // Session Core is owner-bound, so the control is offered only to the author.
  // Showing everyone a button that would be refused every time is a control
  // that lies.
  it('offers the pin control only to the post’s author', async () => {
    renderRoom(
      [
        { thread_id: 'mine', space_id: 'room-1', title: 'Min', owner_subject_id: 'viewer', latest_run_status: 'completed' },
        { thread_id: 'theirs', space_id: 'room-1', title: 'Deres', owner_subject_id: 'other', latest_run_status: 'completed' },
      ],
      [agent],
      { viewerSubjectId: () => 'viewer' },
    )
    await screen.findByText('Deres')
    const buttons = screen.getAllByRole('button', { name: /Fest|Løsne/ })
    expect(buttons.length).toBe(1)
    expect(buttons[0]?.closest('.verevon-room-post')?.textContent).toContain('Min')
  })

  it('pins through the server and re-reads rather than flipping a local copy', async () => {
    const changed = vi.fn()
    renderRoom(
      [{ thread_id: 't1', space_id: 'room-1', title: 'Min', owner_subject_id: 'viewer', latest_run_status: 'completed' }],
      [agent],
      { viewerSubjectId: () => 'viewer', onPresentationChanged: changed },
    )
    ;(await screen.findByRole('button', { name: 'Fest' })).click()
    await waitFor(() =>
      expect(spacesClient.updateSpaceThreadPresentation).toHaveBeenCalledWith('room-1', 't1', { pinned: true }),
    )
    await waitFor(() => expect(changed).toHaveBeenCalled())
    // Still says "Fest": the flag only flips when the server's listing says so.
    expect(screen.getByRole('button', { name: 'Fest' })).toBeTruthy()
  })

  it('offers Unpin on a pinned post and asks the server to unpin', async () => {
    renderRoom(
      [{ thread_id: 't1', space_id: 'room-1', title: 'Min', owner_subject_id: 'viewer', pinned: true, latest_run_status: 'completed' }],
      [agent],
      { viewerSubjectId: () => 'viewer' },
    )
    ;(await screen.findByRole('button', { name: 'Løsne' })).click()
    await waitFor(() =>
      expect(spacesClient.updateSpaceThreadPresentation).toHaveBeenCalledWith('room-1', 't1', { pinned: false }),
    )
  })

  it('says a refused pin changed nothing', async () => {
    spacesClient.updateSpaceThreadPresentation.mockRejectedValue(new Error('owner only'))
    const changed = vi.fn()
    renderRoom(
      [{ thread_id: 't1', space_id: 'room-1', title: 'Min', owner_subject_id: 'viewer', latest_run_status: 'completed' }],
      [agent],
      { viewerSubjectId: () => 'viewer', onPresentationChanged: changed },
    )
    ;(await screen.findByRole('button', { name: 'Fest' })).click()
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText(/Ingenting ble endret/)).toBeTruthy()
    expect(changed).not.toHaveBeenCalled()
  })
})
