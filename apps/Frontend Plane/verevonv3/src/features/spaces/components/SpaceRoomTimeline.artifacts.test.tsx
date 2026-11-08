import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS07-S11 / WS07-S4: room artifacts in the timeline (chips, the per-thread sheet, live
// artifacts, the flag) and the transcript read states. The client is mocked, so no
// request leaves the test and no model is involved.

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
vi.mock('@/features/chat/components/ChatMessages', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')

const agent = { agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active' }
const post = {
  thread_id: 't1',
  space_id: 'room-1',
  title: 'Innkjøp',
  preview: 'Forhåndsvisning',
  latest_run_status: 'completed',
}

const art = (over: Record<string, unknown> = {}) => ({
  id: 'a1',
  kind: 'document',
  title: 'Innkjøpsplan',
  content: 'Dokumentets tekst',
  version: 2,
  ...over,
})

const renderRoom = (extra: Record<string, unknown> = {}, threads: readonly unknown[] = [post]) =>
  render(() => (
    <SpaceRoomTimeline
      spaceRef="room-1"
      spaceName="Lager"
      threads={() => threads as never}
      roster={() => []}
      agents={() => [agent] as never}
      {...(extra as object)}
    />
  ))

const withTurns = (turns: unknown[]) =>
  spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns })

beforeEach(() => {
  vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
  spacesClient.getSpaceThreadTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns: [] })
  spacesClient.updateSpaceThreadPresentation.mockReset()
})

afterEach(() => {
  cleanup()
  vi.unstubAllEnvs()
})

describe('SpaceRoomTimeline retrieval decline (D1-242)', () => {
  it('keeps the one-line note on a reopened thread, under the assistant turn it belongs to', async () => {
    withTurns([
      { role: 'user', content: 'Hva sier policyen?', authorSubjectId: 'user-1' },
      { role: 'assistant', content: 'Det vet jeg ikke.', agentName: 'Lagerhjelpen', retrievalDeclined: 'role_below_floor' },
      { role: 'assistant', content: 'Et annet svar.', agentName: 'Lagerhjelpen' },
    ])
    renderRoom()
    const note = await screen.findByText(/rollen din i rommet tillater ikke kunnskapssøk/)
    expect(note.getAttribute('data-retrieval-declined')).toBe('role_below_floor')
    expect(note.closest('.verevon-room-turn__content')?.textContent).toContain('Det vet jeg ikke.')
    expect(document.querySelectorAll('[data-retrieval-declined]')).toHaveLength(1)
  })

  it('says nothing for a turn that recorded no decline', async () => {
    withTurns([{ role: 'assistant', content: 'Vanlig svar.', agentName: 'Lagerhjelpen' }])
    renderRoom()
    await screen.findByText('Vanlig svar.')
    expect(document.querySelector('[data-retrieval-declined]')).toBeNull()
  })
})

describe('SpaceRoomTimeline artifacts (WS07-S11)', () => {
  it('shows a chip under an assistant turn with the title, kind and version', async () => {
    withTurns([
      { role: 'user', content: 'Lag en plan', authorSubjectId: 'user-1' },
      { role: 'assistant', content: 'Her er planen.', agentName: 'Lagerhjelpen', messageId: 'm2', artifacts: [art()] },
    ])
    renderRoom()
    const chip = await screen.findByRole('button', { name: /Åpne artefakt Innkjøpsplan, versjon 2/ })
    expect(chip.textContent).toContain('Innkjøpsplan')
    expect(chip.textContent).toContain('Dokument')
    expect(chip.textContent).toContain('v2')
    expect(chip.closest('.verevon-space-artifact-chips')).toBeTruthy()
  })

  it('renders an artifact-only assistant turn as a chip with no prose row', async () => {
    withTurns([{ role: 'assistant', content: '', messageId: 'm1', artifacts: [art({ kind: 'code', title: 'script.py' })] }])
    renderRoom()
    const chip = await screen.findByRole('button', { name: /Åpne artefakt script.py/ })
    expect(chip.textContent).toContain('Kode')
  })

  it('opens a dialog sheet with the pressed artifact selected, and Escape closes it and returns focus', async () => {
    withTurns([
      {
        role: 'assistant',
        content: 'To ting.',
        artifacts: [
          art({ id: 'first', title: 'Første', content: 'Første innhold', version: 1 }),
          art({ id: 'second', title: 'Andre', content: 'Andre innhold', version: 1 }),
        ],
      },
    ])
    renderRoom()
    const chip = await screen.findByRole('button', { name: /Åpne artefakt Første/ })
    chip.focus()
    fireEvent.click(chip)
    const dialog = await screen.findByRole('dialog')
    expect(dialog.getAttribute('aria-modal')).toBe('true')
    // The pressed artifact (not the last one) is the one on show.
    await waitFor(() => expect(dialog.textContent).toContain('Første innhold'))
    expect(dialog.textContent).not.toContain('Andre innhold')
    fireEvent.keyDown(dialog, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(chip))
  })

  it('draws html only after Run preview, never straight away', async () => {
    withTurns([
      {
        role: 'assistant',
        content: 'Side.',
        artifacts: [art({ kind: 'html', title: 'Side', content: '<p>hei</p>' })],
      },
    ])
    renderRoom()
    fireEvent.click(await screen.findByRole('button', { name: /Åpne artefakt Side/ }))
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Kjør forhåndsvisning' })).toBeTruthy())
    expect(dialog.querySelector('iframe')).toBeNull()
  })

  it('shows a live artifact as a chip until the transcript carries the same id and version', async () => {
    const live = [art({ id: 'live-1', title: 'Fersk', version: 1 })]
    // The transcript has not persisted it yet.
    withTurns([{ role: 'assistant', content: 'Skriver …' }])
    const first = renderRoom({ liveArtifacts: () => live })
    expect(await screen.findByRole('button', { name: /Åpne artefakt Fersk, versjon 1/ })).toBeTruthy()
    first.unmount()

    // Now the transcript carries the same id+version: exactly one chip, not two.
    withTurns([{ role: 'assistant', content: 'Ferdig.', artifacts: [live[0]] }])
    renderRoom({ liveArtifacts: () => live })
    await screen.findByText('Ferdig.')
    await waitFor(() => expect(screen.getAllByRole('button', { name: /Åpne artefakt Fersk, versjon 1/ })).toHaveLength(1))
  })

  it('keeps a live revision newer than the transcript as a separate chip', async () => {
    withTurns([{ role: 'assistant', content: 'v1', artifacts: [art({ id: 'x', title: 'Plan', version: 1 })] }])
    renderRoom({ liveArtifacts: () => [art({ id: 'x', title: 'Plan', version: 2 })] })
    expect(await screen.findByRole('button', { name: /Åpne artefakt Plan, versjon 1/ })).toBeTruthy()
    expect(await screen.findByRole('button', { name: /Åpne artefakt Plan, versjon 2/ })).toBeTruthy()
  })

  it('with the flag off drops artifact-only turns and shows no chip, live or persisted', async () => {
    vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'false')
    withTurns([
      { role: 'user', content: 'Lag en plan' },
      { role: 'assistant', content: '', artifacts: [art()] },
      { role: 'assistant', content: 'Her er teksten.', artifacts: [art({ id: 'b', title: 'Annen' })] },
    ])
    renderRoom({ liveArtifacts: () => [art({ id: 'live', title: 'Fersk' })] })
    await screen.findByText('Her er teksten.')
    expect(screen.queryByRole('button', { name: /Åpne artefakt/ })).toBeNull()
    expect(document.querySelector('.verevon-space-artifact-chips')).toBeNull()
    expect(screen.queryByRole('dialog')).toBeNull()
    expect(document.querySelectorAll('.verevon-room-turn').length).toBe(2)
  })
})

describe('SpaceRoomTimeline restricted and read states (WS07-S4)', () => {
  it('says once for the room, and makes zero transcript calls, when shared reads are unavailable', async () => {
    renderRoom({ sharedReadAvailable: () => false }, [post, { ...post, thread_id: 't2', title: 'Annen' }])
    expect(await screen.findAllByText(/Samtalene i dette rommet er ikke delt med deg/)).toHaveLength(1)
    expect(spacesClient.loadSpaceTranscript).not.toHaveBeenCalled()
    expect(spacesClient.getSpaceThreadTranscript).not.toHaveBeenCalled()
  })

  it('shows a thread notice for a 403 post and keeps the preview', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'restricted' })
    renderRoom()
    expect(await screen.findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
    expect(screen.getByText('Forhåndsvisning')).toBeTruthy()
  })

  it('says a 404 post is no longer available', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'missing' })
    renderRoom()
    expect(await screen.findByText(/ikke lenger tilgjengelig/)).toBeTruthy()
  })

  it('offers Retry on a 5xx and refetches through loadSpaceTranscript', async () => {
    spacesClient.loadSpaceTranscript.mockResolvedValueOnce({ state: 'error' })
    renderRoom()
    const retry = await screen.findByRole('button', { name: 'Prøv igjen' })
    spacesClient.loadSpaceTranscript.mockResolvedValueOnce({
      state: 'ok',
      threadId: 't1',
      turns: [{ role: 'user', content: 'Nå virker det' }],
    })
    retry.click()
    expect(await screen.findByText('Nå virker det')).toBeTruthy()
    expect(spacesClient.loadSpaceTranscript).toHaveBeenCalledTimes(2)
  })

  it('renders turns on an ok read and never calls the old transcript getter', async () => {
    withTurns([{ role: 'user', content: 'Hva er status?' }])
    renderRoom()
    expect(await screen.findByText('Hva er status?')).toBeTruthy()
    expect(spacesClient.loadSpaceTranscript).toHaveBeenCalledWith('room-1', 't1')
    expect(spacesClient.getSpaceThreadTranscript).not.toHaveBeenCalled()
  })
})
