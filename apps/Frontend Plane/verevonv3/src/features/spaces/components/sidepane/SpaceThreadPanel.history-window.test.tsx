import { cleanup, render, screen, waitFor } from '@solidjs/testing-library'
import { createSignal, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import type { SpaceOwnHistoryWindow } from '@/shared/api/spaces-client'

// D18: the same quiet marker the timeline has, above the turns of the thread open in the side pane, and a pane that
// reads the thread again, rather than keeping what it already holds, when the caller's floor moves. Nothing here asks
// for the turns Control hides: the pane receives what the read gives it.

const spacesClient = vi.hoisted(() => ({
  searchSpaceMessages: vi.fn(() => new Promise(() => {})),
  getSpaceThreadTranscript: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  ...spacesClient,
}))
vi.mock('@/features/chat/components/ChatMessages', () => ({
  ChatMarkdown: (props: { content: string }) => <p>{props.content}</p>,
}))

const { SpaceThreadPanel } = await import('./SpaceThreadPanel')

const hiddenWindow = (over: Partial<SpaceOwnHistoryWindow> = {}): SpaceOwnHistoryWindow => ({
  source: 'member',
  mode: 'since_join',
  sinceAt: null,
  hidden: true,
  ...over,
})

function renderPanel(extra: Record<string, unknown> = {}) {
  render(() => (
    <SpaceThreadPanel
      spaceRef="room-1"
      threadId="thr_1"
      thread={() => ({ thread_id: 'thr_1', space_id: 'room-1', updated_at: '2026-10-27T10:00:00Z' }) as never}
      roster={() => []}
      agentName="Lagerhjelpen"
      {...(extra as object)}
    />
  ))
}

const notes = () => screen.queryAllByRole('note')

beforeEach(() => {
  spacesClient.getSpaceThreadTranscript.mockReset()
})

afterEach(() => cleanup())

describe('SpaceThreadPanel: the hidden-history marker (D18)', () => {
  it('draws the marker above the turns when Control says earlier messages are hidden', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Etter at du ble med' }] })
    renderPanel({ historyWindow: () => hiddenWindow() })
    const turn = await screen.findByText('Etter at du ble med')
    expect(notes()).toHaveLength(1)
    expect(notes()[0]?.textContent).toContain('Tidligere meldinger er skjult')
    expect((notes()[0] as HTMLElement).compareDocumentPosition(turn) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('draws no marker when nothing is hidden, the window is absent, or the thread cannot be read', async () => {
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hei' }] })
    renderPanel({ historyWindow: () => hiddenWindow({ hidden: false }) })
    await screen.findByText('Hei')
    expect(notes()).toHaveLength(0)
    cleanup()
    renderPanel()
    await screen.findByText('Hei')
    expect(notes()).toHaveLength(0)
    cleanup()
    // Not while loading, and not for a thread that is restricted, missing or failed: there is nothing read to be hidden.
    spacesClient.getSpaceThreadTranscript.mockReturnValue(new Promise(() => undefined))
    renderPanel({ historyWindow: () => hiddenWindow() })
    expect(await screen.findByText(/Henter samtalen/)).toBeTruthy()
    expect(notes()).toHaveLength(0)
    cleanup()
    for (const status of [403, 404, 500]) {
      spacesClient.getSpaceThreadTranscript.mockRejectedValue(new ApiError('x', status, null))
      renderPanel({ historyWindow: () => hiddenWindow() })
      await waitFor(() => expect(screen.queryByText(/Henter samtalen/)).toBeNull())
      expect(notes(), String(status)).toHaveLength(0)
      cleanup()
    }
  })

  it('is not drawn with shared reads off, where the restricted notice says the rest', async () => {
    renderPanel({ historyWindow: () => hiddenWindow(), sharedReadAvailable: () => false })
    expect(await screen.findByText(/Samtalene i dette rommet er ikke delt med deg/)).toBeTruthy()
    expect(notes()).toHaveLength(0)
    expect(spacesClient.getSpaceThreadTranscript).not.toHaveBeenCalled()
  })

  it('reads the thread again, and drops what it held, when the caller’s floor moves', async () => {
    const [window, setWindow] = createSignal<SpaceOwnHistoryWindow | undefined>(hiddenWindow({ hidden: false, mode: 'full' }))
    spacesClient.getSpaceThreadTranscript.mockResolvedValueOnce({
      turns: [
        { role: 'user', content: 'Fra før gulvet' },
        { role: 'user', content: 'Etter gulvet' },
      ],
    })
    renderPanel({ historyWindow: window })
    expect(await screen.findByText('Fra før gulvet')).toBeTruthy()
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(1)
    expect(notes()).toHaveLength(0)

    // An admin sets a window: the first read is what this pane holds, and it was read under the old floor.
    let answer: (value: unknown) => void = () => undefined
    spacesClient.getSpaceThreadTranscript.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve }))
    setWindow(hiddenWindow({ mode: 'since_date', sinceAt: new Date(2026, 10, 12, 0, 0, 0).toISOString() }))
    flush()
    await waitFor(() => expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(2))
    // While the new read is in flight the turns read under the old floor are not on screen.
    expect(screen.queryByText('Fra før gulvet')).toBeNull()
    expect(screen.queryByText('Etter gulvet')).toBeNull()
    expect(await screen.findByText(/Henter samtalen/)).toBeTruthy()

    answer({ turns: [{ role: 'user', content: 'Etter gulvet' }] })
    expect(await screen.findByText('Etter gulvet')).toBeTruthy()
    expect(screen.queryByText('Fra før gulvet')).toBeNull()
    expect(notes()).toHaveLength(1)
  })

  it('does not read the thread again for a window that did not move', async () => {
    const [window, setWindow] = createSignal<SpaceOwnHistoryWindow | undefined>(hiddenWindow())
    spacesClient.getSpaceThreadTranscript.mockResolvedValue({ turns: [{ role: 'user', content: 'Hei' }] })
    renderPanel({ historyWindow: window })
    await screen.findByText('Hei')
    // A fresh-but-equal window (the 30 s recheck builds a new object) is the same floor.
    setWindow(hiddenWindow())
    flush()
    await new Promise((resolve) => setTimeout(resolve, 20))
    expect(spacesClient.getSpaceThreadTranscript).toHaveBeenCalledTimes(1)
    expect(screen.getByText('Hei')).toBeTruthy()
  })
})
