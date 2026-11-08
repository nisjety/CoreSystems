import { cleanup, fireEvent, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// WS07-S11 / WS07-S12: room artifacts are held in memory only. A title or body that
// reaches localStorage, sessionStorage, IndexedDB or a cookie, on the live path or
// the persisted path, in the timeline or the Artifacts tab, fails here. The client
// is mocked: no request, no model.

const client = vi.hoisted(() => ({
  getSpaceArtifacts: vi.fn(),
  getSpaceThreadTranscript: vi.fn(),
  loadSpaceTranscript: vi.fn(),
  updateSpaceThreadPresentation: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ...client,
}))
vi.mock('@/features/chat/components/ChatMessages', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))
vi.mock('./SpaceApprovalPanel', () => ({
  SpaceApprovalPanel: () => <div data-testid="approval-panel" />,
}))

const { SpaceRoomTimeline } = await import('./SpaceRoomTimeline')
const { SpaceArtifactsTab } = await import('./SpaceArtifactsTab')
const { ActiveSpaceTabContext } = await import('./SpaceCockpit')
const { createRoomLiveArtifacts } = await import('../lib/room-live-artifacts')

// Markers no UI string, key or id in the app could contain by accident.
const TITLE = 'ZQX-hemmelig-tittel'
const BODY = 'ZQX-hemmelig-innhold'
const LIVE_TITLE = 'ZQX-levende-tittel'
const LIVE_BODY = 'ZQX-levende-innhold'
const MARKERS = [TITLE, BODY, LIVE_TITLE, LIVE_BODY]

const writes: string[] = []
let originalCookie: PropertyDescriptor | undefined

function everythingStored(): string {
  const parts: string[] = [...writes]
  for (const store of [window.localStorage, window.sessionStorage]) {
    for (let index = 0; index < store.length; index += 1) {
      const key = store.key(index) ?? ''
      parts.push(key, store.getItem(key) ?? '')
    }
  }
  parts.push(document.cookie)
  return parts.join('\n')
}

beforeEach(() => {
  vi.stubEnv('VITE_SPACE_ROOM_ARTIFACTS_UI', 'true')
  writes.length = 0
  const realSetItem = Storage.prototype.setItem
  vi.spyOn(Storage.prototype, 'setItem').mockImplementation(function (this: Storage, key: string, value: string) {
    writes.push(String(key), String(value))
    return realSetItem.call(this, key, value)
  })
  originalCookie = Object.getOwnPropertyDescriptor(Document.prototype, 'cookie')
  if (originalCookie?.set && originalCookie.get) {
    const { get, set } = originalCookie
    Object.defineProperty(document, 'cookie', {
      configurable: true,
      get() {
        return get.call(document)
      },
      set(value: string) {
        writes.push(String(value))
        set.call(document, value)
      },
    })
  }
  if (typeof indexedDB !== 'undefined') {
    const realOpen = indexedDB.open.bind(indexedDB)
    vi.spyOn(indexedDB, 'open').mockImplementation((name: string, version?: number) => {
      writes.push(`indexedDB.open:${name}`)
      return realOpen(name, version)
    })
  }
  client.getSpaceArtifacts.mockReset()
  client.getSpaceThreadTranscript.mockReset()
  client.loadSpaceTranscript.mockReset()
  client.updateSpaceThreadPresentation.mockReset()
  client.getSpaceArtifacts.mockResolvedValue({
    artifacts: [
      {
        thread_id: 't1',
        message_id: 'm1',
        artifact_id: 'a1',
        kind: 'document',
        title: TITLE,
        version: 1,
        version_count: 1,
        size_chars: BODY.length,
      },
    ],
    nextCursor: null,
    sharedReadAvailable: true,
    zdr: false,
  })
  client.loadSpaceTranscript.mockResolvedValue({
    state: 'ok',
    threadId: 't1',
    turns: [
      {
        role: 'assistant',
        content: 'Her.',
        messageId: 'm1',
        artifacts: [{ id: 'a1', kind: 'document', title: TITLE, content: BODY, version: 1 }],
      },
    ],
  })
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  vi.unstubAllEnvs()
  if (originalCookie) Object.defineProperty(Document.prototype, 'cookie', originalCookie)
  Reflect.deleteProperty(document, 'cookie')
  window.localStorage.clear()
  window.sessionStorage.clear()
})

const post = { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', preview: 'Forhåndsvisning', latest_run_status: 'completed' }

describe('room artifacts touch no browser storage (WS07-S11, WS07-S12)', () => {
  it('timeline: persisted chip, opened sheet and a live artifact write nothing', async () => {
    const live = createRoomLiveArtifacts()
    live.note({ id: 'live-1', kind: 'code', title: LIVE_TITLE, content: LIVE_BODY, version: 1 }, 't1')
    flush()
    render(() => (
      <SpaceRoomTimeline
        spaceRef="room-1"
        spaceName="Lager"
        threads={() => [post] as never}
        roster={() => []}
        agents={() => []}
        liveArtifacts={(threadId: string) => live.forThread(threadId)}
      />
    ))
    fireEvent.click(await screen.findByRole('button', { name: new RegExp(`Åpne artefakt ${TITLE}`) }))
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(dialog.textContent).toContain(BODY))
    expect(await screen.findByRole('button', { name: new RegExp(`Åpne artefakt ${LIVE_TITLE}`) })).toBeTruthy()
    const stored = everythingStored()
    for (const marker of MARKERS) expect(stored).not.toContain(marker)
  })

  it('Artifacts tab: list, opened sheet and a live artifact write nothing', async () => {
    const live = createRoomLiveArtifacts()
    const [active] = createSignal<'artefakter'>('artefakter')
    render(() => (
      <ActiveSpaceTabContext value={active as never}>
        <SpaceArtifactsTab spaceRef="room-1" live={live} />
      </ActiveSpaceTabContext>
    ))
    await screen.findByText(TITLE)
    live.note({ id: 'live-2', kind: 'document', title: LIVE_TITLE, content: LIVE_BODY, version: 1 }, 't9')
    flush()
    await screen.findByText(LIVE_TITLE)
    fireEvent.click(screen.getByText(TITLE).closest('button')!)
    const dialog = await screen.findByRole('dialog')
    await waitFor(() => expect(dialog.textContent).toContain(BODY))
    const stored = everythingStored()
    for (const marker of MARKERS) expect(stored).not.toContain(marker)
  })

  it('the spy itself sees a write (the detector is not blind)', () => {
    window.localStorage.setItem('probe', `${TITLE}`)
    expect(everythingStored()).toContain(TITLE)
  })
})
