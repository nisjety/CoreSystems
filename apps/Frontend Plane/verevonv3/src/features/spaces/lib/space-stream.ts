import { ApiError } from '@/shared/api/http'
import { readSseStream, type SseEvent } from '@/shared/api/sse'

/**
 * The Space room's live-update controller (WS02-S5).
 *
 * A transport, not a data source: the stream carries ids and closed words only
 * (`src/shared/api/contracts/space-events.json`), so every meaningful frame
 * just asks the page to re-read its own listing. The listing stays the single
 * source of truth; nothing here patches local state.
 *
 * The page owns the fallback: whenever {@link SpaceStreamState} is not `live`
 * the page polls every few seconds, so a disabled, revoked or failing stream
 * degrades to exactly today's behaviour.
 */
export type SpaceStreamState = 'idle' | 'connecting' | 'live' | 'degraded' | 'unavailable'

export type SpaceStreamOptions = {
  spaceRef: string
  /** Ask the page to re-read its listing. Called coalesced, never per frame. */
  onChange: () => void
  onState?: (state: SpaceStreamState) => void
  /** Injectable for tests. */
  random?: () => number
  now?: () => number
}

export type SpaceStreamController = {
  start(): void
  stop(): void
  /**
   * One new attempt after a successful authority recheck, for a stream that
   * ended `forbidden` / `membership_revoked`. A permanent `unavailable`
   * (disabled, zero data retention) stays put for the page session.
   */
  rearm(): void
  state(): SpaceStreamState
}

export const OPEN_TIMEOUT_MS = 8_000
export const SILENCE_WATCHDOG_MS = 30_000
export const HIDDEN_CLOSE_MS = 30_000
export const RESUME_MIN_GAP_MS = 5_000
export const BACKOFF_MIN_MS = 1_000
export const BACKOFF_MAX_MS = 30_000
export const REFETCH_DEBOUNCE_MS = 300
export const REFETCH_JITTER_MS = 700
export const REFETCH_MIN_GAP_MS = 1_500
export const DEGRADED_AFTER_FAILURES = 3

type Cursor = { generation: string; seq: number }

/** `<generation>.<seq>`; anything else is no cursor at all. */
export function parseSpaceStreamCursor(id: string | undefined): Cursor | undefined {
  if (!id) return undefined
  const dot = id.lastIndexOf('.')
  if (dot <= 0 || dot === id.length - 1) return undefined
  const seq = Number(id.slice(dot + 1))
  if (!Number.isSafeInteger(seq) || seq < 0) return undefined
  return { generation: id.slice(0, dot), seq }
}

/** Full-jitter exponential backoff, floored at the contract's 1 s minimum. */
export function spaceStreamBackoffMs(failures: number, random: () => number): number {
  const cap = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** Math.max(0, failures - 1))
  return Math.max(BACKOFF_MIN_MS, Math.floor(random() * cap))
}

type Permanence = 'permanent' | 'rearmable'

// Closing codes that mean the path is unhealthy, as opposed to a routine
// lease renewal. They count toward `degraded`.
const FAILING_CLOSE_CODES = new Set(['upstream_unavailable', 'invalid_frame', 'slow_consumer'])

function asRecord(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined
}

export function createSpaceStreamController(options: SpaceStreamOptions): SpaceStreamController {
  const random = options.random ?? Math.random
  const now = options.now ?? Date.now

  let state: SpaceStreamState = 'idle'
  let running = false
  let unavailable: Permanence | undefined
  let cursor: Cursor | undefined
  let failures = 0
  let attemptId = 0
  let abort: AbortController | undefined
  let handled = false
  let openTimer: number | undefined
  let watchdogTimer: number | undefined
  let reconnectTimer: number | undefined
  let hiddenTimer: number | undefined
  let refetchTimer: number | undefined
  let lastAttemptAt = 0
  let lastRefetchAt = 0
  let refetchPending = false
  let suspended = false

  const isVisible = () => typeof document === 'undefined' || document.visibilityState === 'visible'

  function setState(next: SpaceStreamState): void {
    if (state === next) return
    state = next
    options.onState?.(next)
  }

  function clearTimer(id: number | undefined): undefined {
    if (id !== undefined) window.clearTimeout(id)
    return undefined
  }

  function closeConnection(): void {
    openTimer = clearTimer(openTimer)
    watchdogTimer = clearTimer(watchdogTimer)
    attemptId += 1
    handled = true
    abort?.abort()
    abort = undefined
  }

  // ---- coalesced refetch ---------------------------------------------------

  function flushRefetch(): void {
    refetchTimer = undefined
    if (!running) return
    if (!isVisible()) return // flushed when the tab returns
    refetchPending = false
    lastRefetchAt = now()
    options.onChange()
  }

  function scheduleRefetch(): void {
    refetchPending = true
    if (refetchTimer !== undefined || !running || !isVisible()) return
    const wanted = REFETCH_DEBOUNCE_MS + random() * REFETCH_JITTER_MS
    const wait = Math.max(wanted, lastRefetchAt + REFETCH_MIN_GAP_MS - now())
    refetchTimer = window.setTimeout(flushRefetch, wait)
  }

  // ---- connection lifecycle ------------------------------------------------

  function armWatchdog(id: number): void {
    watchdogTimer = clearTimer(watchdogTimer)
    watchdogTimer = window.setTimeout(() => {
      if (id === attemptId) fail()
    }, SILENCE_WATCHDOG_MS)
  }

  function scheduleReconnect(delayMs: number): void {
    reconnectTimer = clearTimer(reconnectTimer)
    if (!running || suspended || unavailable) return
    reconnectTimer = window.setTimeout(() => {
      reconnectTimer = undefined
      connect()
    }, delayMs)
  }

  function endAttempt(): void {
    closeConnection()
  }

  /** A path failure: back off, and fall back to polling once it persists. */
  function fail(): void {
    if (!running) return
    const wasLive = state === 'live'
    endAttempt()
    failures += 1
    if (wasLive || failures >= DEGRADED_AFTER_FAILURES) setState('degraded')
    else setState('connecting')
    scheduleReconnect(spaceStreamBackoffMs(failures, random))
  }

  function giveUp(kind: Permanence): void {
    endAttempt()
    reconnectTimer = clearTimer(reconnectTimer)
    unavailable = kind
    setState('unavailable')
  }

  /** The server asked for a fresh start (hole, `cursor_too_old`): snapshot, then reconnect without a cursor. */
  function resnapshot(): void {
    endAttempt()
    cursor = undefined
    scheduleRefetch()
    scheduleReconnect(random() * 1_000)
  }

  function handleFrame(id: number, frame: SseEvent): void {
    if (id !== attemptId || !running) return
    let data: Record<string, unknown> | undefined
    try {
      data = asRecord(JSON.parse(frame.data))
    } catch {
      return
    }
    if (!data) return
    openTimer = clearTimer(openTimer)
    armWatchdog(id)

    switch (frame.event) {
      case 'connected': {
        if (data.mode === 'poll_only') {
          // Zero Data Retention (or a ZDR decision): the server serves no rows.
          giveUp('permanent')
          return
        }
        const resumed = data.resumed === true
        if (!resumed) {
          // The head of a fresh stream is the baseline for the next frame.
          cursor = parseSpaceStreamCursor(frame.id) ?? cursor
          scheduleRefetch()
        }
        setState('live')
        return
      }
      case 'space_event': {
        const seq = typeof data.seq === 'number' ? data.seq : Number.NaN
        const generation = data.generation === undefined || data.generation === null ? undefined : String(data.generation)
        const sseCursor = parseSpaceStreamCursor(frame.id)
        if (!Number.isSafeInteger(seq) || generation === undefined) return
        if (cursor && (cursor.generation !== generation || seq !== cursor.seq + 1)) {
          resnapshot()
          return
        }
        cursor = sseCursor && sseCursor.generation === generation && sseCursor.seq === seq ? sseCursor : { generation, seq }
        failures = 0
        // A `hidden` stub keeps the sequence contiguous and tells us nothing
        // we may see; everything else (including run state) is one refetch.
        if (data.kind !== 'hidden') scheduleRefetch()
        return
      }
      case 'heartbeat': {
        failures = 0
        // The cursor is deliberately not advanced from a heartbeat: rows up to
        // `head` may still be in flight, and adopting it would skip them.
        if (cursor && data.generation !== undefined && String(data.generation) !== cursor.generation) resnapshot()
        return
      }
      case 'cursor_too_old': {
        resnapshot()
        return
      }
      case 'closing': {
        const code = typeof data.code === 'string' ? data.code : ''
        const reconnect = data.reconnect === true
        endAttempt()
        if (code === 'zdr_poll_only') {
          unavailable = 'permanent'
          setState('unavailable')
          return
        }
        if (code === 'membership_revoked' || !reconnect) {
          unavailable = 'rearmable'
          setState('unavailable')
          return
        }
        if (FAILING_CLOSE_CODES.has(code)) {
          failures += 1
          if (failures >= DEGRADED_AFTER_FAILURES) setState('degraded')
          scheduleReconnect(spaceStreamBackoffMs(failures, random))
          return
        }
        // Routine (lease renewal, reauthorize, drain): resume from the cursor
        // without leaving `live`, so a renewal is not a flicker.
        const retry = typeof data.retry_after_ms === 'number' && data.retry_after_ms > 0 ? data.retry_after_ms : 0
        scheduleReconnect(retry > 0 ? Math.min(retry, BACKOFF_MAX_MS) : random() * 1_000)
        return
      }
      case 'error': {
        const code = asRecord(data.error)?.code
        if (code === 'space_stream_forbidden' || code === 'unauthorized') giveUp('rearmable')
        else fail()
        return
      }
      default:
        return
    }
  }

  function handleError(id: number, err: unknown): void {
    if (id !== attemptId || !running || handled) return
    if (err instanceof ApiError) {
      // `space_stream_disabled`, or an older gateway with no route at all: stay on the poll.
      if (err.status === 404) return giveUp('permanent')
      if (err.status === 401 || err.status === 403) return giveUp('rearmable')
      if (err.status === 400 && err.code === 'invalid_cursor') {
        cursor = undefined
        scheduleRefetch()
      }
    }
    fail()
  }

  function handleDone(id: number): void {
    // The stream ended without a `closing` frame (a proxy cut, an upstream
    // restart): treat it as a path failure unless we already acted on it.
    if (id !== attemptId || !running || handled) return
    fail()
  }

  function connect(): void {
    reconnectTimer = clearTimer(reconnectTimer)
    if (!running || suspended || unavailable) return
    if (!isVisible()) return
    closeConnection()
    handled = false
    lastAttemptAt = now()
    const id = ++attemptId
    const controller = new AbortController()
    abort = controller
    if (state === 'idle') setState('connecting')
    openTimer = window.setTimeout(() => {
      if (id === attemptId) fail()
    }, OPEN_TIMEOUT_MS)
    armWatchdog(id)
    void readSseStream(
      `/api/v1/spaces/${encodeURIComponent(options.spaceRef)}/events`,
      { method: 'GET', signal: controller.signal, lastEventId: cursor ? `${cursor.generation}.${cursor.seq}` : undefined },
      (frame) => handleFrame(id, frame),
      (err) => handleError(id, err),
      () => handleDone(id),
    )
  }

  // ---- visibility, focus, online -------------------------------------------

  function resume(): void {
    if (!running || unavailable || !isVisible()) return
    suspended = false
    hiddenTimer = clearTimer(hiddenTimer)
    if (refetchPending) scheduleRefetch()
    if (abort && !handled) return // a connection is already open or opening
    if (reconnectTimer !== undefined) return
    const wait = Math.max(0, lastAttemptAt + RESUME_MIN_GAP_MS - now())
    scheduleReconnect(wait)
  }

  function onVisibility(): void {
    if (isVisible()) {
      resume()
      return
    }
    hiddenTimer = clearTimer(hiddenTimer)
    hiddenTimer = window.setTimeout(() => {
      hiddenTimer = undefined
      if (!running || isVisible()) return
      // A background tab is not watching the room: close, and resume on return.
      suspended = true
      reconnectTimer = clearTimer(reconnectTimer)
      refetchTimer = clearTimer(refetchTimer)
      closeConnection()
      if (state === 'live' || state === 'connecting') setState('idle')
    }, HIDDEN_CLOSE_MS)
  }

  return {
    start() {
      if (running) return
      running = true
      unavailable = undefined
      failures = 0
      suspended = false
      if (typeof document !== 'undefined') {
        document.addEventListener('visibilitychange', onVisibility)
        window.addEventListener('focus', resume)
        window.addEventListener('online', resume)
      }
      if (isVisible()) connect()
      else onVisibility()
    },
    stop() {
      if (!running) return
      running = false
      closeConnection()
      reconnectTimer = clearTimer(reconnectTimer)
      hiddenTimer = clearTimer(hiddenTimer)
      refetchTimer = clearTimer(refetchTimer)
      if (typeof document !== 'undefined') {
        document.removeEventListener('visibilitychange', onVisibility)
        window.removeEventListener('focus', resume)
        window.removeEventListener('online', resume)
      }
      setState('idle')
    },
    rearm() {
      if (!running || unavailable !== 'rearmable') return
      unavailable = undefined
      failures = 0
      setState('connecting')
      connect()
    },
    state: () => state,
  }
}
