import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'

// WS07-S12: the room's Artifacts tab. The client is mocked: no request leaves the
// test and no model is involved.

const client = vi.hoisted(() => ({
  getSpaceArtifacts: vi.fn(),
  loadSpaceTranscript: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ...client,
}))
vi.mock('@/features/chat/components/ChatMessages', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))

const { SpaceArtifactsTab } = await import('./SpaceArtifactsTab')
const { ActiveSpaceTabContext } = await import('./SpaceCockpit')
const { createRoomLiveArtifacts } = await import('../lib/room-live-artifacts')

const summary = (over: Record<string, unknown> = {}) => ({
  thread_id: 't1',
  message_id: 'm1',
  artifact_id: 'a1',
  kind: 'document',
  title: 'Innkjøpsplan',
  version: 2,
  version_count: 2,
  size_chars: 40,
  ...over,
})

const page = (over: Record<string, unknown> = {}) => ({
  artifacts: [summary()],
  nextCursor: null,
  sharedReadAvailable: true,
  zdr: false,
  ...over,
})

type TabId = 'chat' | 'artefakter'

function mount(options: { initial?: TabId; live?: ReturnType<typeof createRoomLiveArtifacts> } = {}) {
  const [active, setActive] = createSignal<TabId>(options.initial ?? 'artefakter')
  const view = render(() => (
    <ActiveSpaceTabContext value={active as never}>
      <SpaceArtifactsTab spaceRef="room-1" live={options.live} />
    </ActiveSpaceTabContext>
  ))
  const select = (id: TabId) => {
    setActive(id)
    flush()
  }
  return { ...view, select }
}

const transcriptWith = (turns: unknown[]) => ({ state: 'ok', threadId: 't1', turns })

beforeEach(() => {
  client.getSpaceArtifacts.mockReset()
  client.loadSpaceTranscript.mockReset()
  client.getSpaceArtifacts.mockResolvedValue(page())
  client.loadSpaceTranscript.mockResolvedValue(
    transcriptWith([
      {
        role: 'assistant',
        content: 'Her.',
        messageId: 'm1',
        artifacts: [{ id: 'a1', kind: 'document', title: 'Innkjøpsplan', content: 'Planens tekst', version: 2 }],
      },
    ]),
  )
})

afterEach(() => {
  cleanup()
})

describe('SpaceArtifactsTab fetching (WS07-S12)', () => {
  it('makes no request while the tab is inactive', async () => {
    mount({ initial: 'chat' })
    await Promise.resolve()
    flush()
    expect(client.getSpaceArtifacts).not.toHaveBeenCalled()
  })

  it('reads once on first activation and again on each re-entry', async () => {
    const view = mount({ initial: 'chat' })
    view.select('artefakter')
    await screen.findByText('Innkjøpsplan')
    expect(client.getSpaceArtifacts).toHaveBeenCalledTimes(1)
    expect(client.getSpaceArtifacts).toHaveBeenCalledWith('room-1')
    view.select('chat')
    expect(client.getSpaceArtifacts).toHaveBeenCalledTimes(1)
    view.select('artefakter')
    await waitFor(() => expect(client.getSpaceArtifacts).toHaveBeenCalledTimes(2))
  })

  it('reads on mount when it is the active tab already', async () => {
    mount({ initial: 'artefakter' })
    await screen.findByText('Innkjøpsplan')
    expect(client.getSpaceArtifacts).toHaveBeenCalledTimes(1)
  })
})

describe('SpaceArtifactsTab states (WS07-S12)', () => {
  it('shows loading before the first answer', async () => {
    client.getSpaceArtifacts.mockReturnValue(new Promise(() => {}))
    mount()
    expect((await screen.findByRole('status')).textContent).toContain('Henter artefakter')
  })

  it('says "No artifacts yet" for an empty room', async () => {
    client.getSpaceArtifacts.mockResolvedValue(page({ artifacts: [] }))
    mount()
    expect(await screen.findByText('Ingen artefakter ennå')).toBeTruthy()
  })

  it('shows the room restricted notice for shared_read unavailable and lists nothing', async () => {
    client.getSpaceArtifacts.mockResolvedValue(page({ sharedReadAvailable: false }))
    mount()
    expect(await screen.findByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeTruthy()
    expect(screen.queryByText('Innkjøpsplan')).toBeNull()
  })

  it('shows the restricted notice for a 403', async () => {
    client.getSpaceArtifacts.mockRejectedValue(new ApiError('no', 403, 'space_read_not_authorized'))
    mount()
    expect(await screen.findByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeTruthy()
  })

  it('shows the zero data retention copy and lists nothing', async () => {
    client.getSpaceArtifacts.mockResolvedValue(page({ zdr: true }))
    mount()
    expect(await screen.findByText('Ingen artefakter lagres')).toBeTruthy()
    expect(screen.queryByText('Innkjøpsplan')).toBeNull()
    expect(screen.queryByText('Ingen artefakter ennå')).toBeNull()
  })

  it('shows an error with Retry, not an empty room, and Retry reads again', async () => {
    client.getSpaceArtifacts.mockRejectedValueOnce(new ApiError('boom', 500, null))
    mount()
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('Vi fikk ikke hentet artefaktene')
    expect(screen.queryByText('Ingen artefakter ennå')).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: 'Prøv igjen' }))
    expect(await screen.findByText('Innkjøpsplan')).toBeTruthy()
    expect(client.getSpaceArtifacts).toHaveBeenCalledTimes(2)
  })

  it('says the overview is off for a 404 space_room_artifacts_disabled, not an error', async () => {
    client.getSpaceArtifacts.mockRejectedValue(new ApiError('off', 404, 'space_room_artifacts_disabled'))
    mount()
    expect(await screen.findByText(/ikke slått på for dette miljøet/)).toBeTruthy()
    expect(screen.queryByRole('alert')).toBeNull()
  })

  it('treats a 404 with another code as an error', async () => {
    client.getSpaceArtifacts.mockRejectedValue(new ApiError('gone', 404, 'not_found'))
    mount()
    expect(await screen.findByRole('alert')).toBeTruthy()
  })

  it('lists title, kind, version and the version count', async () => {
    client.getSpaceArtifacts.mockResolvedValue(
      page({
        artifacts: [
          summary({ agent_name: 'Lagerhjelpen' }),
          summary({ artifact_id: 'a2', kind: 'code', title: 'script.py', version: 1, version_count: 1 }),
        ],
      }),
    )
    mount()
    const first = (await screen.findByText('Innkjøpsplan')).closest('button')!
    expect(first.textContent).toContain('Dokument')
    expect(first.textContent).toContain('v2')
    expect(first.textContent).toContain('2 versjoner')
    expect(first.textContent).toContain('Lagerhjelpen')
    const second = screen.getByText('script.py').closest('button')!
    expect(second.textContent).toContain('Kode')
    expect(second.textContent).not.toContain('versjoner')
  })
})

describe('SpaceArtifactsTab paging (WS07-S12)', () => {
  it('Load more appends the next page with the cursor and dedupes by thread and artifact', async () => {
    client.getSpaceArtifacts
      .mockResolvedValueOnce(page({ artifacts: [summary(), summary({ artifact_id: 'a2', title: 'To' })], nextCursor: 'c2' }))
      .mockResolvedValueOnce(
        page({
          artifacts: [summary({ title: 'Duplikat' }), summary({ artifact_id: 'a3', title: 'Tre' }), summary({ thread_id: 't2', title: 'Samme id annen tråd' })],
          nextCursor: null,
        }),
      )
    mount()
    await screen.findByText('To')
    fireEvent.click(screen.getByRole('button', { name: 'Vis flere' }))
    await screen.findByText('Tre')
    expect(client.getSpaceArtifacts).toHaveBeenLastCalledWith('room-1', { cursor: 'c2' })
    expect(screen.queryByText('Duplikat')).toBeNull()
    expect(screen.getByText('Samme id annen tråd')).toBeTruthy()
    expect(document.querySelectorAll('.verevon-space-artifacts__entry')).toHaveLength(4)
    // The last page has no cursor: no button.
    expect(screen.queryByRole('button', { name: 'Vis flere' })).toBeNull()
  })

  it('keeps the list and says so when a later page fails', async () => {
    client.getSpaceArtifacts
      .mockResolvedValueOnce(page({ nextCursor: 'c2' }))
      .mockRejectedValueOnce(new ApiError('boom', 500, null))
    mount()
    await screen.findByText('Innkjøpsplan')
    fireEvent.click(screen.getByRole('button', { name: 'Vis flere' }))
    expect(await screen.findByText(/Vi fikk ikke hentet flere artefakter/)).toBeTruthy()
    expect(screen.getByText('Innkjøpsplan')).toBeTruthy()
  })
})

describe('SpaceArtifactsTab opening an entry (WS07-S12)', () => {
  it('reads that thread transcript once, caches it until refresh, and opens the sheet on that artifact', async () => {
    client.getSpaceArtifacts.mockResolvedValue(
      page({ artifacts: [summary(), summary({ artifact_id: 'a2', title: 'Andre' })] }),
    )
    client.loadSpaceTranscript.mockResolvedValue(
      transcriptWith([
        {
          role: 'assistant',
          content: 'Her.',
          artifacts: [
            { id: 'a1', kind: 'document', title: 'Innkjøpsplan', content: 'Planens tekst', version: 2 },
            { id: 'a2', kind: 'document', title: 'Andre', content: 'Andres tekst', version: 1 },
          ],
        },
      ]),
    )
    mount()
    const entry = (await screen.findByText('Innkjøpsplan')).closest('button')!
    entry.focus()
    fireEvent.click(entry)
    const dialog = await screen.findByRole('dialog')
    expect(client.loadSpaceTranscript).toHaveBeenCalledTimes(1)
    expect(client.loadSpaceTranscript).toHaveBeenCalledWith('room-1', 't1')
    await waitFor(() => expect(dialog.textContent).toContain('Planens tekst'))
    expect(dialog.textContent).not.toContain('Andres tekst')
    fireEvent.keyDown(dialog, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())

    // Another entry of the same thread: cached, no second read.
    fireEvent.click(screen.getByText('Andre').closest('button')!)
    const second = await screen.findByRole('dialog')
    await waitFor(() => expect(second.textContent).toContain('Andres tekst'))
    expect(client.loadSpaceTranscript).toHaveBeenCalledTimes(1)
  })

  it('shows the thread restricted notice on a 403 transcript', async () => {
    client.loadSpaceTranscript.mockResolvedValue({ state: 'restricted' })
    mount()
    fireEvent.click((await screen.findByText('Innkjøpsplan')).closest('button')!)
    expect(await screen.findByText(/Denne samtalen er ikke delt med deg/)).toBeTruthy()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('says the thread is no longer available on a 404 transcript', async () => {
    client.loadSpaceTranscript.mockResolvedValue({ state: 'missing' })
    mount()
    fireEvent.click((await screen.findByText('Innkjøpsplan')).closest('button')!)
    expect(await screen.findByText(/ikke lenger tilgjengelig|finnes ikke lenger/)).toBeTruthy()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('shows an error notice for any other transcript failure', async () => {
    client.loadSpaceTranscript.mockResolvedValue({ state: 'error' })
    mount()
    fireEvent.click((await screen.findByText('Innkjøpsplan')).closest('button')!)
    expect(await screen.findByText(/Vi fikk ikke åpnet artefakten/)).toBeTruthy()
    expect(screen.queryByRole('dialog')).toBeNull()
  })

  it('re-reads the transcript after the tab is re-entered (cache cleared on refresh)', async () => {
    const view = mount()
    fireEvent.click((await screen.findByText('Innkjøpsplan')).closest('button')!)
    await screen.findByRole('dialog')
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    view.select('chat')
    view.select('artefakter')
    await waitFor(() => expect(client.getSpaceArtifacts).toHaveBeenCalledTimes(2))
    fireEvent.click((await screen.findByText('Innkjøpsplan')).closest('button')!)
    await screen.findByRole('dialog')
    expect(client.loadSpaceTranscript).toHaveBeenCalledTimes(2)
  })
})

describe('SpaceArtifactsTab live artifacts (WS07-S12)', () => {
  const liveArt = (over: Record<string, unknown> = {}) => ({
    id: 'a1',
    kind: 'document',
    title: 'Innkjøpsplan',
    content: 'Levende tekst',
    version: 3,
    ...over,
  })

  it('shows a live-only artifact first, marked new', async () => {
    const live = createRoomLiveArtifacts()
    mount({ live })
    await screen.findByText('Innkjøpsplan')
    live.note(liveArt({ id: 'fresh', title: 'Fersk', version: 1 }), 't9')
    flush()
    const entries = await waitFor(() => {
      const found = [...document.querySelectorAll('.verevon-space-artifacts__entry')]
      expect(found.length).toBe(2)
      return found
    })
    expect(entries[0]?.textContent).toContain('Fersk')
    expect(entries[0]?.textContent).toContain('Ny')
  })

  it('shows a newer live revision of a listed artifact, and drops one the aggregate already carries', async () => {
    const live = createRoomLiveArtifacts()
    mount({ live })
    await screen.findByText('Innkjøpsplan')
    live.note(liveArt({ version: 2 }), 't1')
    flush()
    expect(document.querySelectorAll('.verevon-space-artifacts__entry')).toHaveLength(1)
    expect(document.querySelector('.verevon-space-artifacts__live')).toBeNull()
    live.note(liveArt({ version: 3 }), 't1')
    flush()
    await waitFor(() => expect(document.querySelector('.verevon-space-artifacts__live')).toBeTruthy())
    expect(document.querySelectorAll('.verevon-space-artifacts__entry')).toHaveLength(1)
    expect(screen.getByText('v3')).toBeTruthy()
  })

  it('opens the sender’s own live copy when the transcript cannot be read', async () => {
    client.loadSpaceTranscript.mockResolvedValue({ state: 'missing' })
    const live = createRoomLiveArtifacts()
    mount({ live })
    await screen.findByText('Innkjøpsplan')
    live.note(liveArt({ version: 3, content: 'Levende tekst' }), 't1')
    flush()
    await waitFor(() => expect(document.querySelector('.verevon-space-artifacts__live')).toBeTruthy())
    fireEvent.click(screen.getByText('Innkjøpsplan').closest('button')!)
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(dialog.textContent).toContain('Levende tekst'))
  })
})
