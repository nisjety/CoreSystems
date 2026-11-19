import { cleanup, render, screen } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type { SpaceOwnHistoryWindow } from '@/shared/api/spaces-client'

// D18: the quiet marker at the top of the room timeline for a member whose earlier messages are hidden. It follows
// Control's own `hidden: true` for the caller, replaces the broader WS04-S11 notice while it shows, and is never drawn
// beside the restricted notice. Nothing here asks for what is hidden.

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

const agent = { agent_ref: 'a1', subject_id: 'agent-1', name: 'Lagerhjelpen', status: 'active' }
const post = { thread_id: 't1', space_id: 'room-1', title: 'Innkjøp', preview: 'Forhåndsvisning', latest_run_status: 'completed' }

const hiddenWindow = (over: Partial<SpaceOwnHistoryWindow> = {}): SpaceOwnHistoryWindow => ({
  source: 'member',
  mode: 'since_join',
  sinceAt: null,
  hidden: true,
  ...over,
})

const renderRoom = (
  threads: readonly unknown[],
  extra: Record<string, unknown> = {},
) =>
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

const notes = () => screen.queryAllByRole('note')
const historyNotices = () => document.querySelectorAll('.verevon-space-restricted[data-variant="history"]')

beforeEach(() => {
  spacesClient.getSpaceThreadTranscript.mockReset()
  spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [] })
  spacesClient.loadSpaceTranscript.mockReset()
  spacesClient.loadSpaceTranscript.mockResolvedValue({ state: 'ok', threadId: 't1', turns: [] })
})

afterEach(() => cleanup())

describe('SpaceRoomTimeline: the hidden-history marker (D18)', () => {
  it('draws one marker above the posts when Control says earlier messages are hidden', async () => {
    renderRoom([post], { historyWindow: () => hiddenWindow() })
    await screen.findByText('Forhåndsvisning')
    expect(notes()).toHaveLength(1)
    expect(notes()[0]?.textContent).toContain('Tidligere meldinger er skjult')
    const firstPost = document.querySelector('.verevon-room-post') as HTMLElement
    expect((notes()[0] as HTMLElement).compareDocumentPosition(firstPost) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('names the date for a window that starts on one', async () => {
    const noon = new Date(2026, 10, 12, 12, 0, 0).toISOString()
    renderRoom([post], { historyWindow: () => hiddenWindow({ mode: 'since_date', sinceAt: noon }) })
    await screen.findByText('Forhåndsvisning')
    expect(notes()[0]?.textContent).toMatch(/Du ser meldinger fra 12\.? nov\.? 2026\./i)
  })

  it('draws no marker when the window is not hidden, absent or malformed', async () => {
    renderRoom([post], { historyWindow: () => hiddenWindow({ hidden: false }) })
    await screen.findByText('Forhåndsvisning')
    expect(notes()).toHaveLength(0)
    cleanup()
    renderRoom([post], { historyWindow: () => undefined })
    await screen.findByText('Forhåndsvisning')
    expect(notes()).toHaveLength(0)
    cleanup()
    renderRoom([post])
    await screen.findByText('Forhåndsvisning')
    expect(notes()).toHaveLength(0)
  })

  it('follows the window while the room is open: it comes and goes with Control’s answer', async () => {
    const [window, setWindow] = createSignal<SpaceOwnHistoryWindow | undefined>(undefined)
    renderRoom([post], { historyWindow: window })
    await screen.findByText('Forhåndsvisning')
    expect(notes()).toHaveLength(0)
    setWindow(hiddenWindow())
    flush()
    expect(notes()).toHaveLength(1)
    setWindow(hiddenWindow({ hidden: false }))
    flush()
    expect(notes()).toHaveLength(0)
  })

  it('replaces the broader since-join notice instead of standing beside it, and leaves it when nothing is hidden', async () => {
    renderRoom([post], { historyMode: () => 'since_join', historyWindow: () => hiddenWindow() })
    await screen.findByText('Forhåndsvisning')
    expect(notes()).toHaveLength(1)
    expect(historyNotices()).toHaveLength(0)
    cleanup()
    // Nothing hidden by Control's own word: the earlier notice stands exactly as WS04-S11 left it.
    renderRoom([post], { historyMode: () => 'since_join', historyWindow: () => hiddenWindow({ hidden: false }) })
    await screen.findByText('Forhåndsvisning')
    expect(notes()).toHaveLength(0)
    expect(historyNotices()).toHaveLength(1)
  })

  it('is not drawn beside the restricted notice: with shared reads off there is nothing to be hidden from', async () => {
    renderRoom([post], { historyWindow: () => hiddenWindow(), sharedReadAvailable: () => false })
    expect(await screen.findByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeTruthy()
    expect(notes()).toHaveLength(0)
  })

  it('explains an empty room to a member whose earlier messages are hidden, above the intro', async () => {
    renderRoom([], { historyWindow: () => hiddenWindow() })
    expect(await screen.findByRole('note')).toBeTruthy()
    expect(notes()).toHaveLength(1)
    const intro = document.querySelector('.verevon-room-intro') as HTMLElement
    expect(intro).not.toBeNull()
    expect((notes()[0] as HTMLElement).compareDocumentPosition(intro) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('draws no marker in an empty room with nothing hidden', async () => {
    renderRoom([], { historyWindow: () => hiddenWindow({ hidden: false }) })
    await screen.findByRole('heading', { name: 'Lager' })
    expect(notes()).toHaveLength(0)
  })

  it('asks for nothing more because a window is hidden: the marker is the only thing it adds', async () => {
    renderRoom([post], { historyWindow: () => hiddenWindow() })
    await screen.findByText('Forhåndsvisning')
    // The transcript read is the post's own, by room and thread: no floor, date or "show all" is passed along.
    for (const call of spacesClient.loadSpaceTranscript.mock.calls) expect(call).toEqual(['room-1', 't1'])
    expect(spacesClient.searchSpaceMessages).not.toHaveBeenCalled()
    expect(screen.queryByRole('button', { name: /vis|show|skjulte|hidden/i })).toBeNull()
  })
})
