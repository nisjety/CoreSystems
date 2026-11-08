import os
os.chdir("C:/dev/CoresSystem/.claude/worktrees/practical-mccarthy-1dc567/apps/Frontend Plane/verevonv3/src/features/spaces")


def edit(p, pairs):
    b = open(p, "rb").read().decode("utf-8")
    crlf = "\r\n" in b
    b = b.replace("\r\n", "\n")
    for old, new in pairs:
        assert old in b, (p, old[:70])
        b = b.replace(old, new, 1)
    if crlf:
        b = b.replace("\n", "\r\n")
    open(p, "wb").write(b.encode("utf-8"))


NEW_HEAD = """describe('SpaceRoomTimeline — a thread with replies folds to a summary row (WS01-S7b, U2)', () => {
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

"""

p = "components/SpaceRoomTimeline.test.tsx"
b = open(p, "rb").read().decode("utf-8")
crlf = "\r\n" in b
b = b.replace("\r\n", "\n")
i = b.index("describe('SpaceRoomTimeline — long thread collapse and reply count (WS01-S7b)'")
j = b.index("  it('names the reply count on the footer button from reply_count'")
b = b[:i] + NEW_HEAD + b[j:]
if crlf:
    b = b.replace("\n", "\r\n")
open(p, "wb").write(b.encode("utf-8"))

edit("components/SpacePage.test.tsx", [
    ("    it('draws a long thread collapsed in the room only with human posts on', async () => {",
     "    it('folds a thread with replies to a summary row in the room only with human posts on', async () => {"),
    ("      expect(await screen.findByRole('button', { name: '2 meldinger til i tråden' })).toBeTruthy()",
     "      expect(await screen.findByRole('button', { name: 'Åpne tråd · 5 svar' })).toBeTruthy()"),
    ("      expect(screen.getByText('Melding 2')).toBeTruthy()\n      expect(document.querySelector('.verevon-room-post__collapsed')).toBeNull()",
     "      expect(screen.getByText('Melding 2')).toBeTruthy()\n      expect(document.querySelector('.verevon-room-post__summary')).toBeNull()"),
])

p = "lib/space-thread-presentation.test.ts"
b = open(p, "rb").read().decode("utf-8")
print(b[b.index("import {"):b.index("} from './space-thread-presentation'") + 40])
