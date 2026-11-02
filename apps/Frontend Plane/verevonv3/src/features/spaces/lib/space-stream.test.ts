import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import type { SseEvent } from '@/shared/api/sse'

// The transport is stubbed: no network, no model. Each connect attempt is recorded and stays open
// until the test ends it, the way a real stream does.
type SseCall = {
  url: string
  init: { method?: string; signal?: AbortSignal; lastEventId?: string }
  onEvent: (event: SseEvent) => void
  onError: (err: unknown) => void
  onDone: () => void
}
const calls: SseCall[] = []

vi.mock('@/shared/api/sse', () => ({
  readSseStream: (
    url: string,
    init: SseCall['init'],
    onEvent: SseCall['onEvent'],
    onError: SseCall['onError'],
    onDone: SseCall['onDone'],
  ) => {
    calls.push({ url, init, onEvent, onError, onDone })
    return new Promise<void>(() => undefined)
  },
}))

const {
  BACKOFF_MAX_MS,
  DEGRADED_AFTER_FAILURES,
  HIDDEN_CLOSE_MS,
  OPEN_TIMEOUT_MS,
  REFETCH_DEBOUNCE_MS,
  REFETCH_MIN_GAP_MS,
  RESUME_MIN_GAP_MS,
  SILENCE_WATCHDOG_MS,
  createSpaceStreamController,
  parseSpaceStreamCursor,
  spaceStreamBackoffMs,
} = await import('./space-stream')

type Controller = ReturnType<typeof createSpaceStreamController>

const emit = (call: SseCall | undefined, event: string, data: Record<string, unknown>, id?: string) =>
  call?.onEvent({ event, data: JSON.stringify(data), id })

const connectedFrame = (call: SseCall | undefined, extra: Record<string, unknown> = {}) =>
  emit(
    call,
    'connected',
    {
      v: 1, space_id: 'room-1', generation: 'g1', head: 5, resumed: false,
      lease_seconds: 300, heartbeat_seconds: 10, mode: 'live', ...extra,
    },
    'g1.5',
  )

const spaceEvent = (call: SseCall | undefined, seq: number, extra: Record<string, unknown> = {}) => {
  const generation = String(extra.generation ?? 'g1')
  emit(
    call,
    'space_event',
    {
      v: 1, source: 'session', space_id: 'room-1', generation, seq, kind: 'message.appended',
      at: '2026-10-27T10:00:00Z', actor: { type: 'user' }, refs: {}, ...extra,
    },
    `${generation}.${seq}`,
  )
}

const setVisible = (visible: boolean) => {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => (visible ? 'visible' : 'hidden') })
  document.dispatchEvent(new Event('visibilitychange'))
}
// Flip the visible state without the event, for the focus/online resume paths.
const setVisibleSilently = (visible: boolean) => {
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => (visible ? 'visible' : 'hidden') })
}

let controller: Controller | undefined
let onChange: ReturnType<typeof vi.fn<() => void>>
let states: string[]

// `random: () => 0` makes every jittered wait its floor: backoff 1 s, refetch debounce 300 ms,
// resnapshot and routine reconnect 0 ms. Tests that care about jitter pass their own.
function make(overrides: Partial<Parameters<typeof createSpaceStreamController>[0]> = {}): Controller {
  controller = createSpaceStreamController({
    spaceRef: 'room-1',
    onChange: () => onChange(),
    onState: (state) => states.push(state),
    random: () => 0,
    now: () => Date.now(),
    ...overrides,
  })
  return controller
}

const advance = (ms: number) => vi.advanceTimersByTimeAsync(ms)

// Time passing on an open connection: the server's 10 s heartbeats keep the silence watchdog quiet,
// which is what lets a hidden tab reach its own 30 s close timer.
async function advanceWithHeartbeats(ms: number): Promise<void> {
  let left = ms
  while (left > 0) {
    const step = Math.min(10_000, left)
    await advance(step)
    left -= step
    const open = calls.find((call) => call.init.signal?.aborted === false)
    if (open) emit(open, 'heartbeat', { head: 5, generation: 'g1' }, 'g1.5')
  }
}

beforeEach(() => {
  vi.useFakeTimers()
  vi.setSystemTime(1_000_000)
  calls.length = 0
  onChange = vi.fn()
  states = []
  setVisibleSilently(true)
})

afterEach(() => {
  controller?.stop()
  controller = undefined
  vi.useRealTimers()
  setVisibleSilently(true)
})

describe('parseSpaceStreamCursor', () => {
  it('reads <generation>.<seq>', () => {
    expect(parseSpaceStreamCursor('g1.5')).toEqual({ generation: 'g1', seq: 5 })
    expect(parseSpaceStreamCursor('g1.0')).toEqual({ generation: 'g1', seq: 0 })
  })

  it('splits at the last dot, so a generation may itself contain dots', () => {
    expect(parseSpaceStreamCursor('a.b.7')).toEqual({ generation: 'a.b', seq: 7 })
  })

  it('treats anything else as no cursor', () => {
    for (const bad of [undefined, '', 'nodot', '.5', 'g.', 'g.x', 'g.-1', 'g.1.5x', 'g.99999999999999999999']) {
      expect(parseSpaceStreamCursor(bad as string | undefined), String(bad)).toBeUndefined()
    }
  })
})

describe('spaceStreamBackoffMs', () => {
  it('is full jitter up to a doubling cap, floored at the contract minimum of 1 s', () => {
    expect(spaceStreamBackoffMs(1, () => 0)).toBe(1_000)
    expect(spaceStreamBackoffMs(1, () => 0.999)).toBe(1_000)
    expect(spaceStreamBackoffMs(2, () => 0.5)).toBe(1_000)
    expect(spaceStreamBackoffMs(2, () => 0.99)).toBe(1_980)
    expect(spaceStreamBackoffMs(3, () => 0.5)).toBe(2_000)
    expect(spaceStreamBackoffMs(4, () => 0.5)).toBe(4_000)
  })

  it('never exceeds 30 s, however many failures', () => {
    expect(spaceStreamBackoffMs(6, () => 0.999)).toBeLessThanOrEqual(BACKOFF_MAX_MS)
    expect(spaceStreamBackoffMs(6, () => 0.999)).toBe(29_970)
    expect(spaceStreamBackoffMs(500, () => 0.999999)).toBeLessThan(BACKOFF_MAX_MS)
  })

  it('treats zero or negative failures as the first', () => {
    expect(spaceStreamBackoffMs(0, () => 0.999)).toBe(1_000)
    expect(spaceStreamBackoffMs(-3, () => 0.999)).toBe(1_000)
  })
})

describe('connecting', () => {
  it('opens GET /api/v1/spaces/{ref}/events with no cursor, and says it is connecting', () => {
    make({ spaceRef: 'room 1' }).start()
    expect(calls).toHaveLength(1)
    expect(calls[0]?.url).toBe('/api/v1/spaces/room%201/events')
    expect(calls[0]?.init.method).toBe('GET')
    expect(calls[0]?.init.lastEventId).toBeUndefined()
    expect(controller?.state()).toBe('connecting')
  })

  it('goes live on a live connected frame', () => {
    make().start()
    connectedFrame(calls[0])
    expect(controller?.state()).toBe('live')
    expect(states).toEqual(['connecting', 'live'])
  })

  it('start is idempotent', () => {
    make().start()
    controller?.start()
    expect(calls).toHaveLength(1)
  })

  it('ignores a frame that is not JSON or not an object', () => {
    make().start()
    calls[0]?.onEvent({ event: 'connected', data: 'not json' })
    calls[0]?.onEvent({ event: 'connected', data: '[1,2]' })
    expect(controller?.state()).toBe('connecting')
  })

  it('stop closes the connection, goes idle and never reconnects', async () => {
    make().start()
    connectedFrame(calls[0])
    controller?.stop()
    expect(calls[0]?.init.signal?.aborted).toBe(true)
    expect(controller?.state()).toBe('idle')
    await advance(120_000)
    expect(calls).toHaveLength(1)
  })

  it('stop removes the focus and online listeners', async () => {
    make().start()
    controller?.stop()
    window.dispatchEvent(new Event('focus'))
    window.dispatchEvent(new Event('online'))
    await advance(10_000)
    expect(calls).toHaveLength(1)
  })
})

describe('timeouts and failure', () => {
  it('fails an open that has seen no frame for 8 s, then retries after the 1 s backoff', async () => {
    make().start()
    await advance(OPEN_TIMEOUT_MS - 1)
    expect(calls[0]?.init.signal?.aborted).toBe(false)
    await advance(1)
    expect(calls[0]?.init.signal?.aborted).toBe(true)
    expect(controller?.state()).toBe('connecting')
    await advance(999)
    expect(calls).toHaveLength(1)
    await advance(1)
    expect(calls).toHaveLength(2)
  })

  it('a first frame cancels the open timeout', async () => {
    make().start()
    await advance(OPEN_TIMEOUT_MS - 1_000)
    connectedFrame(calls[0])
    await advance(OPEN_TIMEOUT_MS)
    expect(calls).toHaveLength(1)
    expect(controller?.state()).toBe('live')
  })

  it('30 s of silence on a live stream degrades it at once and reconnects', async () => {
    make().start()
    connectedFrame(calls[0])
    await advance(SILENCE_WATCHDOG_MS - 1)
    expect(controller?.state()).toBe('live')
    await advance(1)
    expect(controller?.state()).toBe('degraded')
    expect(calls[0]?.init.signal?.aborted).toBe(true)
    await advance(1_000)
    expect(calls).toHaveLength(2)
  })

  it('every frame, a heartbeat included, restarts the 30 s watchdog', async () => {
    make().start()
    connectedFrame(calls[0])
    await advance(20_000)
    emit(calls[0], 'heartbeat', { head: 5, generation: 'g1' }, 'g1.5')
    await advance(SILENCE_WATCHDOG_MS - 1)
    expect(controller?.state()).toBe('live')
    await advance(1)
    expect(controller?.state()).toBe('degraded')
  })

  it('three consecutive failures degrade a stream that never went live', async () => {
    make().start()
    expect(DEGRADED_AFTER_FAILURES).toBe(3)
    calls[0]?.onError(new Error('network'))
    expect(controller?.state()).toBe('connecting')
    await advance(1_000)
    calls[1]?.onError(new Error('network'))
    expect(controller?.state()).toBe('connecting')
    await advance(1_000)
    expect(calls).toHaveLength(3)
    calls[2]?.onError(new Error('network'))
    expect(controller?.state()).toBe('degraded')
  })

  it('a failure from live is degraded at once, not after three', () => {
    make().start()
    connectedFrame(calls[0])
    calls[0]?.onError(new Error('network'))
    expect(controller?.state()).toBe('degraded')
  })

  it('a stream that ends with no closing frame is a failure', () => {
    make().start()
    connectedFrame(calls[0])
    calls[0]?.onDone()
    expect(controller?.state()).toBe('degraded')
  })

  it('ignores callbacks from an attempt that has been replaced', async () => {
    make().start()
    calls[0]?.onError(new Error('network'))
    await advance(1_000)
    expect(calls).toHaveLength(2)
    // The first attempt's late error must not count against the second.
    calls[0]?.onError(new Error('late'))
    calls[0]?.onDone()
    await advance(5_000)
    expect(calls).toHaveLength(2)
    expect(controller?.state()).toBe('connecting')
  })

  it('goes live again from degraded when a later attempt connects', async () => {
    make().start()
    connectedFrame(calls[0])
    calls[0]?.onDone()
    await advance(1_000)
    connectedFrame(calls[1])
    expect(controller?.state()).toBe('live')
    expect(states).toEqual(['connecting', 'live', 'degraded', 'live'])
  })

  it('an error frame that is not an authority refusal is a path failure', () => {
    make().start()
    connectedFrame(calls[0])
    emit(calls[0], 'error', { error: { code: 'space_stream_unavailable', message: 'x' } })
    expect(controller?.state()).toBe('degraded')
  })

  it('an invalid_cursor 400 drops the cursor and asks for a snapshot', async () => {
    make().start()
    connectedFrame(calls[0])
    spaceEvent(calls[0], 6)
    calls[0]?.onError(new ApiError('bad cursor', 400, 'invalid_cursor'))
    await advance(2_000)
    expect(calls[1]?.init.lastEventId).toBeUndefined()
    expect(onChange).toHaveBeenCalled()
  })
})

describe('a hidden tab', () => {
  it('opens no connection while hidden, and connects when the tab returns', async () => {
    setVisibleSilently(false)
    make().start()
    expect(calls).toHaveLength(0)
    setVisible(true)
    await advance(0)
    expect(calls).toHaveLength(1)
  })

  it('closes the stream after 30 s hidden and resumes with the cursor when visible again', async () => {
    make().start()
    connectedFrame(calls[0])
    setVisible(false)
    await advanceWithHeartbeats(HIDDEN_CLOSE_MS - 1)
    expect(calls[0]?.init.signal?.aborted).toBe(false)
    expect(controller?.state()).toBe('live')
    await advance(1)
    expect(calls[0]?.init.signal?.aborted).toBe(true)
    expect(controller?.state()).toBe('idle')
    setVisible(true)
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBe('g1.5')
  })

  it('a quick hide and return keeps the same connection', async () => {
    make().start()
    connectedFrame(calls[0])
    setVisible(false)
    await advanceWithHeartbeats(10_000)
    setVisible(true)
    await advanceWithHeartbeats(HIDDEN_CLOSE_MS * 2)
    expect(calls).toHaveLength(1)
    expect(controller?.state()).toBe('live')
  })

  it('resumes on focus and on online as well as on visibility', async () => {
    for (const type of ['focus', 'online']) {
      calls.length = 0
      make().start()
      connectedFrame(calls[0])
      setVisible(false)
      await advanceWithHeartbeats(HIDDEN_CLOSE_MS)
      expect(controller?.state()).toBe('idle')
      setVisibleSilently(true)
      window.dispatchEvent(new Event(type))
      await advance(0)
      expect(calls, type).toHaveLength(2)
      controller?.stop()
    }
  })

  it('does not resume while the tab is still hidden', async () => {
    make().start()
    connectedFrame(calls[0])
    setVisible(false)
    await advanceWithHeartbeats(HIDDEN_CLOSE_MS)
    window.dispatchEvent(new Event('focus'))
    window.dispatchEvent(new Event('online'))
    await advance(10_000)
    expect(calls).toHaveLength(1)
  })

  it('keeps resumes at least 5 s after the last attempt', async () => {
    let clock = 1_000
    make({ now: () => clock }).start()
    connectedFrame(calls[0])
    setVisible(false)
    await advanceWithHeartbeats(HIDDEN_CLOSE_MS)
    // The controller's own clock says only 2 s have passed since it connected.
    clock = 3_000
    setVisible(true)
    await advance(RESUME_MIN_GAP_MS - 2_000 - 1)
    expect(calls).toHaveLength(1)
    await advance(1)
    expect(calls).toHaveLength(2)
  })

  it('a resume does not open a second connection while one is open', async () => {
    make().start()
    connectedFrame(calls[0])
    window.dispatchEvent(new Event('focus'))
    window.dispatchEvent(new Event('online'))
    await advance(10_000)
    expect(calls).toHaveLength(1)
  })
})

describe('coalescing frames into one re-read', () => {
  // `resumed: true` so the connection itself asks for no refetch; the frames below are the only cause.
  const live = () => {
    make().start()
    connectedFrame(calls[0], { resumed: true })
  }

  it('turns a burst of frames into one onChange after the debounce', async () => {
    live()
    for (let seq = 1; seq <= 5; seq += 1) spaceEvent(calls[0], seq)
    await advance(REFETCH_DEBOUNCE_MS - 1)
    expect(onChange).not.toHaveBeenCalled()
    await advance(1)
    expect(onChange).toHaveBeenCalledTimes(1)
    await advance(10_000)
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('adds up to the jitter on top of the debounce', async () => {
    make({ random: () => 0.5 }).start()
    connectedFrame(calls[0], { resumed: true })
    spaceEvent(calls[0], 1)
    // 300 + 0.5 * 700 = 650 ms.
    await advance(649)
    expect(onChange).not.toHaveBeenCalled()
    await advance(1)
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('keeps successive re-reads at least 1.5 s apart', async () => {
    live()
    spaceEvent(calls[0], 1)
    await advance(REFETCH_DEBOUNCE_MS)
    expect(onChange).toHaveBeenCalledTimes(1)
    await advance(100)
    spaceEvent(calls[0], 2)
    // The first read was at t = 300 ms; the next may not come before t = 1800 ms.
    await advance(REFETCH_MIN_GAP_MS - 100 - REFETCH_DEBOUNCE_MS - 1)
    expect(onChange).toHaveBeenCalledTimes(1)
    await advance(REFETCH_DEBOUNCE_MS + 1)
    expect(onChange).toHaveBeenCalledTimes(2)
  })

  it('does not re-read for a hidden stub, which tells the member nothing', async () => {
    live()
    spaceEvent(calls[0], 1, { kind: 'hidden' })
    await advance(5_000)
    expect(onChange).not.toHaveBeenCalled()
  })

  it('re-reads for a redacted row and for run state', async () => {
    live()
    spaceEvent(calls[0], 1, { kind: 'redacted' })
    await advance(2_000)
    spaceEvent(calls[0], 2, { kind: 'run.state_changed' })
    await advance(2_000)
    expect(onChange).toHaveBeenCalledTimes(2)
  })

  it('reads only while the tab is visible: a frame that arrives hidden waits for the return', async () => {
    live()
    setVisible(false)
    spaceEvent(calls[0], 1)
    await advanceWithHeartbeats(HIDDEN_CLOSE_MS - 10_000)
    expect(onChange).not.toHaveBeenCalled()
    setVisible(true)
    await advance(REFETCH_DEBOUNCE_MS)
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('stop drops a pending re-read', async () => {
    live()
    spaceEvent(calls[0], 1)
    controller?.stop()
    await advance(5_000)
    expect(onChange).not.toHaveBeenCalled()
  })

  it('a connected frame with resumed:false re-reads once, a resumed one never does', async () => {
    make().start()
    connectedFrame(calls[0], { resumed: false })
    await advance(5_000)
    expect(onChange).toHaveBeenCalledTimes(1)

    controller?.stop()
    calls.length = 0
    onChange.mockClear()
    make().start()
    connectedFrame(calls[0], { resumed: true })
    await advance(5_000)
    expect(onChange).not.toHaveBeenCalled()
  })
})

describe('sequence and generation', () => {
  const liveAtFive = () => {
    make().start()
    connectedFrame(calls[0])
    // The fresh stream's head is the baseline: g1.5. Take the refetch the connection itself asks for.
    return advance(REFETCH_DEBOUNCE_MS).then(() => onChange.mockClear())
  }

  it('accepts the next contiguous seq and resumes from it with Last-Event-ID', async () => {
    await liveAtFive()
    spaceEvent(calls[0], 6)
    spaceEvent(calls[0], 7)
    emit(calls[0], 'closing', { code: 'lease_expired', reconnect: true, retry_after_ms: 0 }, 'g1.7')
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBe('g1.7')
  })

  it('a seq hole snapshots, then reconnects without Last-Event-ID', async () => {
    await liveAtFive()
    spaceEvent(calls[0], 7)
    expect(calls[0]?.init.signal?.aborted).toBe(true)
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBeUndefined()
    // The snapshot is a re-read like any other, so it keeps the 1.5 s gap from the one before.
    await advance(REFETCH_MIN_GAP_MS)
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('a repeated seq is also a break in contiguity', async () => {
    await liveAtFive()
    spaceEvent(calls[0], 6)
    spaceEvent(calls[0], 6)
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBeUndefined()
  })

  it('a new generation snapshots and reconnects without Last-Event-ID', async () => {
    await liveAtFive()
    spaceEvent(calls[0], 6, { generation: 'g2' })
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBeUndefined()
    await advance(REFETCH_MIN_GAP_MS)
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('a heartbeat from another generation snapshots too, but never advances the cursor', async () => {
    await liveAtFive()
    emit(calls[0], 'heartbeat', { head: 40, generation: 'g1' }, 'g1.40')
    spaceEvent(calls[0], 6)
    expect(calls).toHaveLength(1)
    emit(calls[0], 'heartbeat', { head: 1, generation: 'g9' }, 'g9.1')
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBeUndefined()
  })

  it('cursor_too_old snapshots and reconnects without Last-Event-ID', async () => {
    await liveAtFive()
    spaceEvent(calls[0], 6)
    emit(calls[0], 'cursor_too_old', { reason: 'retention', head: 90, generation: 'g1' })
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBeUndefined()
    await advance(REFETCH_MIN_GAP_MS)
    expect(onChange).toHaveBeenCalledTimes(1)
  })

  it('a resnapshot does not leave live: it is a re-read, not an outage', async () => {
    await liveAtFive()
    spaceEvent(calls[0], 9)
    await advance(0)
    expect(controller?.state()).toBe('live')
  })

  it('ignores a space_event that does not carry a numeric seq and a generation', async () => {
    await liveAtFive()
    emit(calls[0], 'space_event', { kind: 'message.appended', generation: 'g1' }, 'g1.6')
    emit(calls[0], 'space_event', { kind: 'message.appended', seq: 6 }, 'g1.6')
    await advance(5_000)
    expect(calls).toHaveLength(1)
    expect(onChange).not.toHaveBeenCalled()
  })
})

describe('closing frames', () => {
  const live = () => {
    make().start()
    connectedFrame(calls[0])
  }

  it('lease renewal reconnects from the cursor without leaving live', async () => {
    live()
    emit(calls[0], 'closing', { code: 'lease_expired', reconnect: true, retry_after_ms: 0 }, 'g1.5')
    expect(controller?.state()).toBe('live')
    await advance(0)
    expect(calls).toHaveLength(2)
    expect(calls[1]?.init.lastEventId).toBe('g1.5')
    expect(controller?.state()).toBe('live')
    expect(states).toEqual(['connecting', 'live'])
  })

  it('honours retry_after_ms, capped at 30 s', async () => {
    live()
    emit(calls[0], 'closing', { code: 'reauthorize', reconnect: true, retry_after_ms: 2_000 })
    await advance(1_999)
    expect(calls).toHaveLength(1)
    await advance(1)
    expect(calls).toHaveLength(2)

    emit(calls[1], 'closing', { code: 'server_draining', reconnect: true, retry_after_ms: 600_000 })
    await advance(BACKOFF_MAX_MS - 1)
    expect(calls).toHaveLength(2)
    await advance(1)
    expect(calls).toHaveLength(3)
  })

  it('a failing close code backs off and counts toward degraded', async () => {
    live()
    emit(calls[0], 'closing', { code: 'upstream_unavailable', reconnect: true, retry_after_ms: 0 })
    expect(controller?.state()).toBe('live')
    await advance(1_000)
    expect(calls).toHaveLength(2)
    emit(calls[1], 'closing', { code: 'slow_consumer', reconnect: true, retry_after_ms: 0 })
    await advance(1_000)
    emit(calls[2], 'closing', { code: 'invalid_frame', reconnect: true, retry_after_ms: 0 })
    expect(controller?.state()).toBe('degraded')
    await advance(1_000)
    expect(calls).toHaveLength(4)
  })

  it('zdr_poll_only is permanently unavailable', async () => {
    live()
    emit(calls[0], 'closing', { code: 'zdr_poll_only', reconnect: false })
    expect(controller?.state()).toBe('unavailable')
    await advance(120_000)
    window.dispatchEvent(new Event('online'))
    controller?.rearm()
    await advance(120_000)
    expect(calls).toHaveLength(1)
  })

  it('a connected frame in poll_only mode is permanently unavailable', async () => {
    make().start()
    connectedFrame(calls[0], { mode: 'poll_only' })
    expect(controller?.state()).toBe('unavailable')
    controller?.rearm()
    await advance(120_000)
    expect(calls).toHaveLength(1)
    expect(calls[0]?.init.signal?.aborted).toBe(true)
  })

  it('membership_revoked is rearmable and rearm makes exactly one new attempt', async () => {
    live()
    emit(calls[0], 'closing', { code: 'membership_revoked', reconnect: false })
    expect(controller?.state()).toBe('unavailable')
    await advance(120_000)
    expect(calls).toHaveLength(1)
    controller?.rearm()
    expect(calls).toHaveLength(2)
    expect(controller?.state()).toBe('connecting')
    controller?.rearm()
    expect(calls).toHaveLength(2)
  })

  it('any other closing with reconnect:false is rearmable too', async () => {
    live()
    emit(calls[0], 'closing', { code: 'something_new', reconnect: false })
    expect(controller?.state()).toBe('unavailable')
    controller?.rearm()
    expect(calls).toHaveLength(2)
  })
})

describe('refusals at the door', () => {
  it('404 (disabled, or an older gateway) is permanently unavailable: no retry, no rearm', async () => {
    make().start()
    calls[0]?.onError(new ApiError('missing', 404, 'space_stream_disabled'))
    expect(controller?.state()).toBe('unavailable')
    await advance(120_000)
    window.dispatchEvent(new Event('online'))
    window.dispatchEvent(new Event('focus'))
    controller?.rearm()
    await advance(120_000)
    expect(calls).toHaveLength(1)
  })

  it('401 and 403 are rearmable, never retried on their own', async () => {
    for (const status of [401, 403]) {
      calls.length = 0
      make().start()
      calls[0]?.onError(new ApiError('no', status, 'space_stream_forbidden'))
      expect(controller?.state(), String(status)).toBe('unavailable')
      await advance(120_000)
      expect(calls, String(status)).toHaveLength(1)
      controller?.rearm()
      expect(calls, String(status)).toHaveLength(2)
      expect(controller?.state()).toBe('connecting')
      controller?.stop()
    }
  })

  it('an error frame with space_stream_forbidden or unauthorized is rearmable', () => {
    for (const code of ['space_stream_forbidden', 'unauthorized']) {
      calls.length = 0
      make().start()
      connectedFrame(calls[0])
      emit(calls[0], 'error', { error: { code, message: 'x' } })
      expect(controller?.state(), code).toBe('unavailable')
      controller?.rearm()
      expect(calls, code).toHaveLength(2)
      controller?.stop()
    }
  })

  it('rearm does nothing on a stream that is not unavailable, or is stopped', () => {
    make().start()
    controller?.rearm()
    expect(calls).toHaveLength(1)
    controller?.stop()
    controller?.rearm()
    expect(calls).toHaveLength(1)
  })

  it('a rearmed stream that connects is live again', () => {
    make().start()
    calls[0]?.onError(new ApiError('no', 403, 'space_stream_forbidden'))
    controller?.rearm()
    connectedFrame(calls[1])
    expect(controller?.state()).toBe('live')
  })
})
