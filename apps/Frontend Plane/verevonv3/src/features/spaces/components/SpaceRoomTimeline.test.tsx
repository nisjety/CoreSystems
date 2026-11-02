import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const spacesClient = vi.hoisted(() => ({
  getSpaceThreadTranscript: vi.fn(),
  loadSpaceTranscript: vi.fn(),
  updateSpaceThreadPresentation: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  // The pure normalisers the transcript list calls stay real; only the network functions are mocked.
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
  spacesClient.loadSpaceTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns: [] })
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

// WS01-S6: the sender's own posts, drawn from the send journal before the room's projection carries them.
describe('SpaceRoomTimeline — pending posts (WS01-S6)', () => {
  const entry = (overrides: Record<string, unknown> = {}) => ({
    sendId: 'send-1',
    kind: 'post_new',
    spaceRef: 'room-1',
    content: 'Hei fra meg',
    stage: 'sending',
    createdAt: 1,
    ...overrides,
  })

  it('draws a pending new post in an otherwise empty room instead of the intro', async () => {
    renderRoom([], [agent], { pendingPosts: () => [entry()] })
    const list = await screen.findByRole('list', { name: 'Samtaler i rommet' })
    expect(list.textContent).toContain('Hei fra meg')
    expect(list.textContent).toContain('Sender …')
  })

  it('draws a pending new post as the last post, after the room’s own', async () => {
    renderRoom(
      [{ thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', preview: 'Eksisterende', latest_run_status: 'completed' }],
      [agent],
      { pendingPosts: () => [entry()] },
    )
    await screen.findByText('Eksisterende')
    const posts = [...document.querySelectorAll('.verevon-room-timeline > li')]
    expect(posts).toHaveLength(2)
    expect(posts[0]?.textContent).toContain('Eksisterende')
    expect(posts[1]?.classList.contains('verevon-room-post--pending')).toBe(true)
    expect(posts[1]?.textContent).toContain('Hei fra meg')
  })

  it.each([
    ['queued', 'Sendes når du er på nett igjen'],
    ['sending', 'Sender …'],
    ['accepted', 'Sendt'],
    ['failed', 'Ikke sendt, prøv igjen'],
  ])('says %s in words', async (stage, text) => {
    renderRoom([], [agent], { pendingPosts: () => [entry({ stage })] })
    const state = await screen.findByText(text)
    expect(state.closest('[data-stage]')?.getAttribute('data-stage')).toBe(stage)
  })

  it('gives a failed row the alert role and every other stage a status', async () => {
    renderRoom([], [agent], { pendingPosts: () => [entry({ stage: 'failed' })] })
    const failed = await screen.findByText('Ikke sendt, prøv igjen')
    expect(failed.getAttribute('role')).toBe('alert')
    cleanup()
    renderRoom([], [agent], { pendingPosts: () => [entry({ stage: 'sending' })] })
    expect((await screen.findByText('Sender …')).getAttribute('role')).toBe('status')
  })

  it('draws a pending reply after the thread’s own turns, and only on its thread', async () => {
    spacesClient.loadSpaceTranscript.mockImplementation(async (_spaceRef: string, threadId: string) => ({
      state: 'ok',
      threadId,
      turns: threadId === 't1'
        ? [
            { role: 'user', content: 'Første' },
            { role: 'assistant', content: 'Assistentens oppsummering' },
          ]
        : [],
    }))
    renderRoom(
      [
        { thread_id: 't1', space_id: 'room-1', title: 'En', latest_run_status: 'completed' },
        { thread_id: 't2', space_id: 'room-1', title: 'To', preview: 'Annen', latest_run_status: 'completed' },
      ],
      [agent],
      { pendingPosts: () => [entry({ kind: 'post_reply', threadId: 't1', content: 'Mitt svar' })] },
    )
    const reply = await screen.findByText('Mitt svar')
    const answer = await screen.findByText('Assistentens oppsummering')
    // After the real turns in document order, inside the same post.
    expect(answer.compareDocumentPosition(reply) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(reply.closest('.verevon-room-post')).toBe(answer.closest('.verevon-room-post'))
    expect(screen.getAllByText('Mitt svar')).toHaveLength(1)
  })

  it('draws nothing extra without a journal', async () => {
    renderRoom([{ thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', preview: 'Eksisterende', latest_run_status: 'completed' }])
    await screen.findByText('Eksisterende')
    expect(document.querySelector('.verevon-room-post--pending')).toBeNull()
    expect(document.querySelector('.verevon-room-turn--pending')).toBeNull()
  })
})

// WS01-S7a: the side pane's entry point and the per-post read states.
describe('SpaceRoomTimeline — thread pane entry and read states (WS01-S7a)', () => {
  const post = { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', preview: 'Forhåndsvisning', latest_run_status: 'completed' }

  it('offers Open thread on every post only when the pane exists, and hands over the thread', async () => {
    const open = vi.fn()
    renderRoom([post], [agent], { onOpenThread: open })
    const button = await screen.findByRole('button', { name: /Åpne tråd/ })
    button.click()
    expect(open).toHaveBeenCalledTimes(1)
    expect(open.mock.calls[0]?.[0]).toMatchObject({ thread_id: 't1' })
  })

  it('has no Open thread control while the pane is off', async () => {
    renderRoom([post])
    await screen.findByText('Forhåndsvisning')
    expect(screen.queryByRole('button', { name: /Åpne tråd/ })).toBeNull()
  })

  it('shows the restricted notice on a 403 and keeps the preview', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'restricted' })
    renderRoom([post])
    expect(await screen.findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
    expect(screen.getByText('Forhåndsvisning')).toBeTruthy()
    expect(document.body.textContent ?? '').not.toMatch(/logg inn/i)
  })

  it('says a thread that is gone is no longer available', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'missing' })
    renderRoom([post])
    expect(await screen.findByText(/ikke lenger tilgjengelig/)).toBeTruthy()
  })

  it('falls back to the preview with a retry on any other failure, never the restricted notice', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValueOnce({ state: 'error' })
    renderRoom([post])
    expect(await screen.findByText('Forhåndsvisning')).toBeTruthy()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Kunne ikke hente denne samtalen')
    expect(screen.queryByText(/ikke delt med deg/)).toBeNull()
    spacesClient.loadSpaceTranscript.mockResolvedValueOnce({
      state: 'ok',
      threadId: 't1',
      turns: [{ role: 'user', content: 'Nå virker det' }],
    })
    ;(screen.getByRole('button', { name: 'Prøv igjen' })).click()
    expect(await screen.findByText('Nå virker det')).toBeTruthy()
  })

  it('makes no transcript request when shared reads are off, and says so once for the room', async () => {
    renderRoom([post], [agent], { sharedReadAvailable: () => false })
    expect(await screen.findByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeTruthy()
    expect(spacesClient.loadSpaceTranscript).not.toHaveBeenCalled()
  })

  it('still renders turns after the turn list extraction: author, text and agent name', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({
      state: 'ok',
      threadId: 't1',
      turns: [
        { role: 'user', content: 'Hva er status?', authorSubjectId: 'user-1' },
        { role: 'assistant', content: 'Alt er klart.', agentName: 'Lagerhjelpen' },
      ],
    })
    renderRoom([post], [agent])
    expect(await screen.findByText('Alt er klart.')).toBeTruthy()
    expect(screen.getByText('Hva er status?')).toBeTruthy()
    expect(screen.getAllByText('Lagerhjelpen').length).toBeGreaterThan(0)
  })
})

describe('SpaceRoomTimeline — what the stylesheet keys on', () => {
  const posts = [
    { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', preview: 'Første', latest_run_status: 'completed' },
    { thread_id: 't2', space_id: 'room-1', title: 'Levering', preview: 'Andre', latest_run_status: 'running' },
  ]

  // The open thread's post stays marked while the pane is up (a wash and an accent edge in the sheet).
  it('marks only the post whose thread is open in the pane', async () => {
    renderRoom(posts, [agent], { onOpenThread: vi.fn(), openThreadId: () => 't2' })
    await screen.findByText('Andre')
    const rows = document.querySelectorAll('.verevon-room-post')
    expect(rows.length).toBe(2)
    expect(rows[0]?.hasAttribute('data-open')).toBe(false)
    expect(rows[1]?.getAttribute('data-open')).toBe('true')
  })

  it('marks no post open while no pane is open', async () => {
    renderRoom(posts, [agent], { onOpenThread: vi.fn(), openThreadId: () => null })
    await screen.findByText('Andre')
    expect(document.querySelectorAll('[data-open]').length).toBe(0)
  })

  // A finished run's status line is quiet in the sheet; the attribute is what it keys on.
  it('carries the run status on the post so a finished post can be quiet', async () => {
    renderRoom(posts)
    await screen.findByText('Andre')
    const rows = document.querySelectorAll('.verevon-room-post')
    expect(rows[0]?.getAttribute('data-run-status')).toBe('completed')
    expect(rows[1]?.getAttribute('data-run-status')).toBe('running')
  })
})

describe('SpaceRoomTimeline — a thread with replies folds to a summary row (WS01-S7b, U2)', () => {
  const post = { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', latest_run_status: 'completed' }
  const turnsOf = (n: number) =>
    Array.from({ length: n }, (_, i) => ({ role: 'user', content: `Melding ${i + 1}`, authorSubjectId: 'user-1' }))
  const transcriptOf = (n: number) =>
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns: turnsOf(n) })
  const on = () => true
  const summary = () => document.querySelector('.verevon-room-post__summary') as HTMLElement | null

  it('shows only the root message and one summary row for a six-turn thread, and the row opens the pane', async () => {
    transcriptOf(6)
    const open = vi.fn()
    renderRoom([post], [agent], { onOpenThread: open, collapseLongThreads: on })

    expect(await screen.findByText('Melding 1')).toBeTruthy()
    for (const n of [2, 3, 4, 5]) expect(screen.queryByText(`Melding ${n}`)).toBeNull()
    // The latest reply is a glance inside the summary row, not a turn of its own.
    expect(summary()?.querySelector('.verevon-room-post__summary-last')?.textContent).toContain('Melding 6')

    const row = screen.getByRole('button', { name: 'Åpne tråd · 5 svar' })
    expect(row.className).toContain('verevon-room-post__summary')
    row.click()
    expect(open).toHaveBeenCalledTimes(1)
    expect(open.mock.calls[0]?.[0]).toMatchObject({ thread_id: 't1' })
  })

  it('folds a two-turn thread too, in the singular', async () => {
    transcriptOf(2)
    renderRoom([post], [agent], { onOpenThread: vi.fn(), collapseLongThreads: on })
    expect(await screen.findByRole('button', { name: 'Åpne tråd · 1 svar' })).toBeTruthy()
    expect(screen.getByText('Melding 1')).toBeTruthy()
    expect(summary()?.querySelector('.verevon-room-post__summary-count')?.textContent).toBe('1 svar')
  })

  it('does not fold a thread of one turn: no summary row, and the footer opens it', async () => {
    transcriptOf(1)
    renderRoom([post], [agent], { onOpenThread: vi.fn(), collapseLongThreads: on })
    expect(await screen.findByText('Melding 1')).toBeTruthy()
    expect(summary()).toBeNull()
    expect(screen.getByRole('button', { name: 'Åpne tråd' })).toBeTruthy()
  })

  it('draws every turn with the flag off', async () => {
    transcriptOf(6)
    renderRoom([post], [agent], { onOpenThread: vi.fn(), collapseLongThreads: () => false })
    expect(await screen.findByText('Melding 6')).toBeTruthy()
    for (const n of [1, 2, 3, 4, 5]) expect(screen.getByText(`Melding ${n}`)).toBeTruthy()
    expect(summary()).toBeNull()
  })

  it('draws every turn with the flag on but no pane to open (onOpenThread absent)', async () => {
    transcriptOf(6)
    renderRoom([post], [agent], { collapseLongThreads: on })
    expect(await screen.findByText('Melding 6')).toBeTruthy()
    for (const n of [1, 2, 3, 4, 5]) expect(screen.getByText(`Melding ${n}`)).toBeTruthy()
    expect(summary()).toBeNull()
  })

  it('names who replied, the latest reply and its author in the summary row', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({
      state: 'ok',
      threadId: 't1',
      turns: [
        { role: 'user', content: 'Rot', authorSubjectId: 'user-1' },
        { role: 'user', content: 'Første svar', authorSubjectId: 'user-2' },
        { role: 'assistant', content: 'Alt er **klart**.', agentName: 'Lagerhjelpen' },
        { role: 'user', content: 'Andre svar fra Ola', authorSubjectId: 'user-2' },
      ],
    })
    const roster = [
      { subject_type: 'user', subject_id: 'user-1', role: 'owner', revision: 1, display_name: 'Kari Nordmann' },
      { subject_type: 'user', subject_id: 'user-2', role: 'editor', revision: 1, display_name: 'Ola Hansen' },
    ]
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [post] as never}
        roster={() => roster as never}
        agents={() => [agent] as never}
        onOpenThread={vi.fn()}
        collapseLongThreads={on}
      />
    ))
    await screen.findByText('Rot')
    const last = summary()?.querySelector('.verevon-room-post__summary-last')
    expect(last?.querySelector('strong')?.textContent).toBe('Ola Hansen')
    expect(last?.textContent).toContain('Andre svar fra Ola')
    // Two distinct repliers (Ola once, the agent once), not one avatar per turn.
    expect(summary()?.querySelectorAll('.verevon-room-post__summary-avatars > *').length).toBe(2)
    expect(screen.getByRole('button', { name: 'Åpne tråd · 3 svar' })).toBeTruthy()
  })

  it('draws at most three avatars and counts the rest', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({
      state: 'ok',
      threadId: 't1',
      turns: [
        { role: 'user', content: 'Rot', authorSubjectId: 'u0' },
        ...['u1', 'u2', 'u3', 'u4', 'u5'].map((id) => ({ role: 'user', content: `Svar ${id}`, authorSubjectId: id })),
      ],
    })
    renderRoom([post], [agent], { onOpenThread: vi.fn(), collapseLongThreads: on })
    await screen.findByText('Rot')
    const stack = summary()?.querySelector('.verevon-room-post__summary-avatars') as HTMLElement
    expect(stack.querySelectorAll('.verevon-room-turn__avatar').length).toBe(3)
    expect(stack.querySelector('.verevon-room-post__summary-more')?.textContent).toBe('+2')
  })

  it('names the reply count on the footer button from reply_count', async () => {
    renderRoom([{ ...post, reply_count: 3 }], [agent], { onOpenThread: vi.fn(), collapseLongThreads: on })
    expect(await screen.findByRole('button', { name: 'Åpne tråd · 3 svar' })).toBeTruthy()
  })

  it('uses the singular for one reply', async () => {
    renderRoom([{ ...post, reply_count: 1 }], [agent], { onOpenThread: vi.fn(), collapseLongThreads: on })
    expect(await screen.findByRole('button', { name: 'Åpne tråd · 1 svar' })).toBeTruthy()
  })

  it('keeps the plain footer label with no replies or with the flag off', async () => {
    const view = renderRoom([{ ...post, reply_count: 0 }], [agent], { onOpenThread: vi.fn(), collapseLongThreads: on })
    expect(await screen.findByRole('button', { name: 'Åpne tråd' })).toBeTruthy()
    view.unmount()
    renderRoom([{ ...post, reply_count: 5 }], [agent], { onOpenThread: vi.fn(), collapseLongThreads: () => false })
    expect(await screen.findByRole('button', { name: 'Åpne tråd' })).toBeTruthy()
  })
})

describe('SpaceRoomTimeline — room pins (WS13-S8)', () => {
  const threads = [
    { thread_id: 'a', space_id: 'room-1', title: 'Alfa', updated_at: '2026-09-07T08:00:00Z', latest_run_status: 'completed' },
    { thread_id: 'b', space_id: 'room-1', title: 'Beta', updated_at: '2026-09-07T09:00:00Z', latest_run_status: 'completed' },
    { thread_id: 'c', space_id: 'room-1', title: 'Gamma', updated_at: '2026-09-07T10:00:00Z', latest_run_status: 'completed' },
    { thread_id: 'd', space_id: 'room-1', title: 'Delta', updated_at: '2026-09-07T11:00:00Z', latest_run_status: 'completed' },
  ]
  const postOrder = () =>
    [...document.querySelectorAll('.verevon-room-post[data-thread-id]')].map((node) => node.getAttribute('data-thread-id'))

  it('lifts pinned posts above the time order, newest pin first', async () => {
    renderRoom(threads, [agent], {
      roomPins: () => [
        { thread_id: 'a', pinned_by: 'user-1', pinned_at: 100 },
        { thread_id: 'c', pinned_by: 'user-2', pinned_at: 200 },
      ],
    })
    await screen.findByText('Alfa')
    // c was pinned last so it leads, then a; the unpinned posts keep their time order.
    expect(postOrder()).toEqual(['c', 'a', 'b', 'd'])
  })

  it('says who pinned a post, by roster name', async () => {
    renderRoom(threads, [agent], {
      roster: () => [{ subject_type: 'user', subject_id: 'user-2', role: 'editor', revision: 1, display_name: 'Kari' }],
      roomPins: () => [{ thread_id: 'c', pinned_by: 'user-2', pinned_at: 200 }],
    })
    expect(await screen.findByText('Festet av Kari')).toBeTruthy()
    expect(document.querySelector('.verevon-room-post[data-thread-id="c"][data-pinned]')).toBeTruthy()
    expect(document.querySelector('.verevon-room-post[data-thread-id="a"][data-pinned]')).toBeNull()
  })

  it('names a pinner who has left the roster without showing the raw id', async () => {
    renderRoom(threads, [agent], {
      roomPins: () => [{ thread_id: 'c', pinned_by: 'user-raw-id-77', pinned_at: 200 }],
    })
    expect(await screen.findByText('Festet av Tidligere medlem')).toBeTruthy()
    expect(document.body.textContent).not.toContain('user-raw-id-77')
  })

  it('leaves the order and flags alone when there are no pins', async () => {
    renderRoom(threads, [agent], { roomPins: () => [] })
    await screen.findByText('Alfa')
    expect(postOrder()).toEqual(['a', 'b', 'c', 'd'])
    expect(document.querySelector('.verevon-room-post__flag--pinned')).toBeNull()
    cleanup()
    renderRoom(threads)
    await screen.findByText('Alfa')
    expect(postOrder()).toEqual(['a', 'b', 'c', 'd'])
  })

  it('ignores a pin whose thread is not in the listing', async () => {
    renderRoom(threads, [agent], { roomPins: () => [{ thread_id: 'gone', pinned_by: 'user-2', pinned_at: 999 }] })
    await screen.findByText('Alfa')
    expect(postOrder()).toEqual(['a', 'b', 'c', 'd'])
  })
})
