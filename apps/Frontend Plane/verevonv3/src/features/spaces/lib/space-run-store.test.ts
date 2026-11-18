import { createRoot, flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type {
  RoomRunFrame,
  RoomRunSnapshot,
  SpaceRunLiveEnd,
  SpaceRunLiveHandlers,
  SpaceRunLiveOptions,
} from '@/shared/api/space-run-live-client'

import { MAX_LIVE_ATTACHES, createSpaceRunStore, type SpaceRunStore, type SpaceRunStream } from './space-run-store'

// WS09-S5: one live stream per run, shared by every viewer in this browser. The transport is a fake that
// records each attach and stays open until the test ends it, the way a real stream does. The store is
// built with `random` at 0.5, so every jittered pause is exactly its base, and time is faked.

type Attach = {
  readonly spaceRef: string
  readonly runId: string
  readonly handlers: SpaceRunLiveHandlers
  readonly options: SpaceRunLiveOptions
}

let attaches: Attach[] = []
const stores: SpaceRunStore[] = []

const stream: SpaceRunStream = (spaceRef, runId, handlers, options) => {
  attaches.push({ spaceRef, runId, handlers, options })
  return new Promise<void>(() => undefined)
}

const forRun = (runId: string): Attach[] => attaches.filter((attach) => attach.runId === runId)

function makeStore(): SpaceRunStore {
  const created = createRoot(() => createSpaceRunStore({ stream, random: () => 0.5 }))
  stores.push(created)
  return created
}

const snapshotFrame = (overrides: Partial<RoomRunSnapshot> = {}): RoomRunFrame => ({
  type: 'snapshot',
  snapshot: {
    runId: 'run-1',
    threadId: 'thread-1',
    startedBy: 'Kari',
    agentName: 'Assistant',
    phase: 'running',
    startedAt: 1_700_000_000,
    text: { offset: 0, value: '', truncated: false, visible: true },
    steps: [],
    steer: 0,
    plan: null,
    artifacts: [],
    expiresAt: null,
    ...overrides,
  },
})

const failed = (status: number, code: string | null): SpaceRunLiveEnd => ({
  kind: 'failed',
  status,
  code,
  feature: null,
})

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(1_700_000_000_000)
  attaches = []
  stores.length = 0
})

afterEach(() => {
  for (const live of stores) live.dispose()
  stores.length = 0
  vi.useRealTimers()
})

describe('the live store: one stream per run (WS09-S5)', () => {
  it('shares one stream between two viewers of the same run', () => {
    const live = makeStore()
    const first = live.attach('room-a', 'run-1')
    const second = live.attach('room-a', 'run-1')
    flush()

    expect(forRun('run-1')).toHaveLength(1)
    expect(forRun('run-1')[0]).toMatchObject({ spaceRef: 'room-a', runId: 'run-1' })
    expect(live.view('run-1')?.status).toBe('connecting')
    first()
    second()
  })

  it('keeps the stream while one viewer is left, and stops it and drops the view when the last lets go', () => {
    const live = makeStore()
    const first = live.attach('room-a', 'run-1')
    const second = live.attach('room-a', 'run-1')
    flush()

    first()
    flush()
    expect(forRun('run-1')[0]?.options.signal?.aborted).toBe(false)
    expect(live.view('run-1')).toBeDefined()

    second()
    flush()
    expect(forRun('run-1')[0]?.options.signal?.aborted).toBe(true)
    expect(live.view('run-1')).toBeUndefined()
    expect(live.views()).toEqual([])

    second()
    flush()
    expect(live.views()).toEqual([])
  })

  it('has at most three streams open at once, and a fourth run waits for a slot', () => {
    expect(MAX_LIVE_ATTACHES).toBe(3)
    const live = makeStore()
    const releases = ['run-1', 'run-2', 'run-3', 'run-4'].map((runId) => live.attach('room-a', runId))
    flush()

    expect(attaches.map((attach) => attach.runId)).toEqual(['run-1', 'run-2', 'run-3'])
    expect(live.view('run-4')?.status).toBe('waiting')
    vi.advanceTimersByTime(60_000)
    expect(attaches).toHaveLength(3)

    releases[1]?.()
    flush()
    expect(attaches.map((attach) => attach.runId)).toEqual(['run-1', 'run-2', 'run-3', 'run-4'])
    expect(live.view('run-4')?.status).toBe('connecting')
  })
})

describe('the live store: re-attaching (WS09-S5)', () => {
  it('closes the stream on a reauth frame and attaches again after a short pause, without the last event id', () => {
    const live = makeStore()
    const release = live.attach('room-a', 'run-1')
    flush()
    const first = forRun('run-1')[0] as Attach
    first.handlers.onFrame(snapshotFrame(), 'g1.1')
    first.handlers.onFrame({ type: 'reauth_required' }, 'g1.2')
    flush()
    expect(first.options.signal?.aborted).toBe(true)

    first.handlers.onEnd({ kind: 'aborted' })
    flush()
    expect(forRun('run-1')).toHaveLength(1)
    vi.advanceTimersByTime(374)
    expect(forRun('run-1')).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(forRun('run-1')).toHaveLength(2)
    expect(forRun('run-1')[1]?.options.lastEventId).toBeUndefined()
    flush()
    expect(live.view('run-1')?.reauthRequired).toBe(false)
    release()
  })

  it('closes the stream on a gap in the text and attaches again, healing from a fresh snapshot', () => {
    const live = makeStore()
    const release = live.attach('room-a', 'run-1')
    flush()
    const first = forRun('run-1')[0] as Attach
    first.handlers.onFrame(snapshotFrame({ text: { offset: 0, value: 'Hel', truncated: false, visible: true } }), 'g1.1')
    first.handlers.onFrame({ type: 'text', offset: 40, delta: 'late' }, 'g1.2')
    flush()
    expect(first.options.signal?.aborted).toBe(true)

    first.handlers.onEnd({ kind: 'aborted' })
    flush()
    vi.advanceTimersByTime(375)
    expect(forRun('run-1')).toHaveLength(2)
    expect(forRun('run-1')[1]?.options.lastEventId).toBeUndefined()
    expect(live.view('run-1')?.needsReattach).toBe(false)
    release()
  })
})

describe('the live store: an expiry before the first snapshot (WS09-S5)', () => {
  it('retries a 410 that comes before any snapshot once, quietly, after a short pause', () => {
    const live = makeStore()
    const release = live.attach('room-a', 'run-1')
    flush()
    forRun('run-1')[0]?.handlers.onEnd(failed(410, 'run_stream_expired'))
    flush()
    expect(live.view('run-1')?.status).toBe('connecting')

    vi.advanceTimersByTime(799)
    expect(forRun('run-1')).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(forRun('run-1')).toHaveLength(2)

    forRun('run-1')[1]?.handlers.onFrame(snapshotFrame(), 'g2.1')
    flush()
    expect(live.view('run-1')?.status).toBe('live')
    release()
  })

  it('gives the legacy row back with the typed code when the retry expires too, and tries nothing more', () => {
    const live = makeStore()
    const release = live.attach('room-a', 'run-1')
    flush()
    forRun('run-1')[0]?.handlers.onEnd(failed(410, 'run_stream_expired'))
    vi.advanceTimersByTime(800)
    forRun('run-1')[1]?.handlers.onEnd(failed(410, 'run_stream_expired'))
    flush()

    expect(live.view('run-1')).toMatchObject({ status: 'unavailable', unavailableCode: 'run_stream_expired' })
    vi.advanceTimersByTime(60_000)
    expect(forRun('run-1')).toHaveLength(2)
    release()
  })

  it('does not retry a 410 that comes after the run was live: it is final for this attach', () => {
    const live = makeStore()
    const release = live.attach('room-a', 'run-1')
    flush()
    forRun('run-1')[0]?.handlers.onFrame(snapshotFrame(), 'g1.1')
    flush()
    forRun('run-1')[0]?.handlers.onEnd(failed(410, 'run_stream_expired'))
    flush()

    expect(live.view('run-1')).toMatchObject({ status: 'unavailable', unavailableCode: 'run_stream_expired' })
    vi.advanceTimersByTime(60_000)
    expect(forRun('run-1')).toHaveLength(1)
    release()
  })
})

describe('the live store: final answers (WS09-S5)', () => {
  it.each([
    ['feature_unavailable', 404],
    ['zdr_feature_not_supported', 403],
    ['space_read_not_authorized', 403],
    ['active_membership_required', 403],
    ['run_not_found', 404],
    ['too_many_live_views', 429],
  ])('treats %s as final: the view is unavailable with that code, and nothing is retried', (code, status) => {
    const live = makeStore()
    const release = live.attach('room-a', 'run-1')
    flush()
    forRun('run-1')[0]?.handlers.onEnd(failed(status, code))
    flush()

    expect(live.view('run-1')).toMatchObject({ status: 'unavailable', unavailableCode: code })
    vi.advanceTimersByTime(60_000)
    expect(forRun('run-1')).toHaveLength(1)
    release()
  })
})

describe('the live store: network failures (WS09-S5)', () => {
  it('retries a network failure with doubling pauses, and after five failed attaches shows the view as unavailable', () => {
    const live = makeStore()
    const release = live.attach('room-a', 'run-1')
    flush()
    const network = failed(0, null)

    forRun('run-1')[0]?.handlers.onEnd(network)
    flush()
    const pauses = [800, 1_600, 3_200, 6_400]
    for (const [index, pause] of pauses.entries()) {
      vi.advanceTimersByTime(pause - 1)
      expect(forRun('run-1')).toHaveLength(index + 1)
      vi.advanceTimersByTime(1)
      expect(forRun('run-1')).toHaveLength(index + 2)
      forRun('run-1')[index + 1]?.handlers.onEnd(network)
      flush()
    }

    expect(forRun('run-1')).toHaveLength(5)
    expect(live.view('run-1')).toMatchObject({ status: 'unavailable', unavailableCode: 'run_live_unavailable' })
    vi.advanceTimersByTime(60_000)
    expect(forRun('run-1')).toHaveLength(5)
    release()
  })
})
