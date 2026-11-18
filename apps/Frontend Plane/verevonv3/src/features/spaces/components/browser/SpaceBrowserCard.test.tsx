import { cleanup, render, screen } from '@solidjs/testing-library'
import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  SpaceBrowserEvent,
  SpaceBrowserRun,
  SpaceBrowserRunHeader,
  SpaceBrowserTimeline,
} from '@/shared/api/spaces-client/browser-runs'
import { SPACE_BROWSER_POLL_MS } from '../../lib/space-browser'
import { clearSpaceRoomFeatures, primeSpaceRoomFeatures } from '../../lib/space-room-features'

// The room's client answers, mocked as the data the fixed client returns. The evidence URL is the real builder, watched:
// a test asserts whether the card ever asked for one.
const browserRuns = vi.hoisted(() => ({
  readSpaceBrowserTimeline: vi.fn(),
  listSpaceBrowserRuns: vi.fn(),
  spaceBrowserEvidenceUrl: vi.fn(),
}))
vi.mock('@/shared/api/spaces-client/browser-runs', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/shared/api/spaces-client/browser-runs')>()
  browserRuns.spaceBrowserEvidenceUrl.mockImplementation(actual.spaceBrowserEvidenceUrl)
  return { ...actual, ...browserRuns }
})

const { SpaceBrowserCard } = await import('./SpaceBrowserCard')
const { SpaceBrowserThreadSection } = await import('./SpaceBrowserThreadSection')

const SPACE = 'room-a'
const RUN_ID = 'run_1'
const STARTER = 'sub_kari'
const OTHER = 'sub_ola'

function header(phase: string): SpaceBrowserRunHeader {
  return {
    runId: RUN_ID,
    threadId: 'thr_1',
    startedBySubjectId: STARTER,
    phase,
    startedAt: '2026-11-19T09:00:00Z',
    finishedAt: null,
    controlMode: 'agent',
    handoffOpen: false,
    viewScope: 'owner',
  }
}

/** The room's list row (R2) for the run: the header, the hosts and the step count. */
const listRun: SpaceBrowserRun = { ...header('running'), hosts: ['shop.example.test'], stepCount: 2 }

const navigated: SpaceBrowserEvent = {
  eventId: 'evt_1',
  at: '2026-11-19T09:00:05Z',
  kind: 'action',
  actionType: 'navigate',
  host: 'shop.example.test',
}
/** A screenshot row names its evidence; the image is the starter's alone (SD-23). */
const screenshot: SpaceBrowserEvent = {
  eventId: 'evt_2',
  at: '2026-11-19T09:00:09Z',
  kind: 'action',
  actionType: 'screenshot',
  evidenceId: 'ev_1',
}

/** One timeline answer (R3) for the run in the given phase. */
function timelineAnswer(phase: string, events: readonly SpaceBrowserEvent[]): SpaceBrowserTimeline {
  return { run: header(phase), events, nextEventId: null, validUntil: '2026-11-19T09:05:00Z' }
}

// jsdom has no IntersectionObserver. The stub reports the card's visibility after observe(), as a browser does. The test
// decides what the first report says, and can report a change later.
let firstSightVisible = true
let deliverVisibility: ((visible: boolean) => void) | undefined
class StubIntersectionObserver {
  callback: (entries: IntersectionObserverEntry[]) => void

  constructor(callback: (entries: IntersectionObserverEntry[]) => void) {
    this.callback = callback
  }

  observe() {
    const report = (visible: boolean) => this.callback([{ isIntersecting: visible } as IntersectionObserverEntry])
    deliverVisibility = report
    queueMicrotask(() => report(firstSightVisible))
  }

  disconnect() {}
}

/** Lets the card's pending answers land, then applies the signal writes they made. */
async function settle() {
  for (let round = 0; round < 3; round += 1) {
    await vi.advanceTimersByTimeAsync(0)
    flush()
  }
}

function mountCard(viewerSubjectId: string, run: SpaceBrowserRun = listRun) {
  render(() => (
    <SpaceBrowserCard
      spaceRef={SPACE}
      run={run}
      viewerSubjectId={viewerSubjectId}
      startedByName="Kari"
      onOpen={vi.fn()}
    />
  ))
  flush()
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'setInterval', 'clearInterval', 'Date'] })
  vi.setSystemTime(new Date('2026-11-19T09:00:00Z'))
  vi.clearAllMocks()
  firstSightVisible = true
  deliverVisibility = undefined
  browserRuns.readSpaceBrowserTimeline.mockReset()
  browserRuns.readSpaceBrowserTimeline.mockResolvedValue(timelineAnswer('running', [navigated, screenshot]))
  browserRuns.listSpaceBrowserRuns.mockReset()
})

afterEach(() => {
  cleanup()
  clearSpaceRoomFeatures()
  vi.unstubAllGlobals()
  vi.useRealTimers()
})

describe('SpaceBrowserCard (WS20-S5)', () => {
  it('a non-starter renders no img and builds no evidence URL', async () => {
    mountCard(OTHER)
    await settle()

    // The room still sees the steps (receipts are for everyone), so the timeline was read.
    expect(screen.getByText('Åpnet side · shop.example.test')).toBeTruthy()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalled()
    expect(document.querySelector('img')).toBeNull()
    expect(browserRuns.spaceBrowserEvidenceUrl).not.toHaveBeenCalled()
    expect(document.body.innerHTML).not.toContain('/evidence/')
  })

  it('the starter gets the img', async () => {
    mountCard(STARTER)
    await settle()

    expect(browserRuns.spaceBrowserEvidenceUrl).toHaveBeenCalledWith(SPACE, RUN_ID, 'ev_1')
    const img = document.querySelector('img')
    expect(img?.getAttribute('src')).toBe('/api/v1/spaces/room-a/browser-runs/run_1/evidence/ev_1')
    expect(img?.getAttribute('alt')).toBe('Siste skjermbilde fra kjøringen')
  })

  it('polling stops at a terminal phase', async () => {
    browserRuns.readSpaceBrowserTimeline
      .mockResolvedValueOnce(timelineAnswer('running', [navigated]))
      .mockResolvedValueOnce(timelineAnswer('completed', []))
    mountCard(OTHER)
    await settle()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalledTimes(1)

    await vi.advanceTimersByTimeAsync(SPACE_BROWSER_POLL_MS)
    flush()
    expect(screen.getByText('Fullført')).toBeTruthy()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalledTimes(2)
    // The second poll asked only for the rows after the last one the card already held.
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenLastCalledWith(SPACE, RUN_ID, { after: 'evt_1', limit: 200 })

    await vi.advanceTimersByTimeAsync(SPACE_BROWSER_POLL_MS * 4)
    flush()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalledTimes(2)
  })

  it('a hidden card does not poll', async () => {
    firstSightVisible = false
    vi.stubGlobal('IntersectionObserver', StubIntersectionObserver)
    mountCard(OTHER)
    await settle()
    await vi.advanceTimersByTimeAsync(SPACE_BROWSER_POLL_MS * 4)
    flush()
    expect(browserRuns.readSpaceBrowserTimeline).not.toHaveBeenCalled()

    // Visible again: it reads at once, then on each interval.
    deliverVisibility?.(true)
    await settle()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalledTimes(1)
    await vi.advanceTimersByTimeAsync(SPACE_BROWSER_POLL_MS)
    flush()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalledTimes(2)
  })

  it('a card that turns hidden stops polling', async () => {
    vi.stubGlobal('IntersectionObserver', StubIntersectionObserver)
    mountCard(OTHER)
    await settle()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalledTimes(1)

    deliverVisibility?.(false)
    flush()
    await vi.advanceTimersByTimeAsync(SPACE_BROWSER_POLL_MS * 3)
    flush()
    expect(browserRuns.readSpaceBrowserTimeline).toHaveBeenCalledTimes(1)
  })
})

describe('the client answers fill the card (WS20-S5)', () => {
  it('a list answer fills the card and its timeline fills the steps', async () => {
    primeSpaceRoomFeatures({ browser_run: true, browser_card: true })
    browserRuns.listSpaceBrowserRuns.mockResolvedValue({ runs: [listRun], nextCursor: null })
    render(() => (
      <SpaceBrowserThreadSection
        spaceRef={SPACE}
        threadId="thr_1"
        canStart={true}
        viewerSubjectId={OTHER}
        nameOf={() => 'Kari'}
        onOpen={vi.fn()}
      />
    ))
    flush()
    await settle()

    expect(browserRuns.listSpaceBrowserRuns).toHaveBeenCalledWith(SPACE, { threadId: 'thr_1', limit: 5 })
    expect(screen.getByText('Startet av Kari · 2 steg')).toBeTruthy()
    expect(screen.getByText('Kjører')).toBeTruthy()
    expect(screen.getByText('shop.example.test')).toBeTruthy()
    expect(screen.getByText('Åpnet side · shop.example.test')).toBeTruthy()
  })
})
