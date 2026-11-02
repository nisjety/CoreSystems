import { createEffect, createSignal, onCleanup } from 'solid-js'

import {
  streamSpaceRunLive,
  type RoomRunFrame,
  type SpaceRunLiveEnd,
  type SpaceRunLiveHandlers,
  type SpaceRunLiveOptions,
} from '@/shared/api/space-run-live-client'
import { applyRoomRunFrame, emptySpaceRunView, type SpaceRunView } from './space-run-view'

/**
 * One live stream per run, shared by every surface that shows it (WS09-S5).
 *
 * The room timeline's run card and the Work and Activity tabs all read the same view of a run, so
 * opening the Work tab while the card is on screen does not open a second stream. Rules:
 *
 * - at most `MAX_LIVE_ATTACHES` (3) streams are open from this browser at once; a fourth run waits for
 *   a slot (status `waiting`, the legacy row shows meanwhile). The server also caps per user and per
 *   run, this is the polite client half;
 * - a stream is released when its last viewer goes away, and a finished run's view is kept only while
 *   something still shows it;
 * - a dropped stream, a `reauth_required` frame (the attach authorization ends after two minutes) and
 *   a text gap all re-attach with a jittered pause, because every attach re-snapshots and heals;
 * - the first moments of a turn can race the server's run pointer (410 before the first snapshot): one
 *   quiet retry before giving the legacy row back;
 * - `feature_unavailable`, a Zero Data Retention refusal, a lost audience and a full house are final for
 *   this attach: status `unavailable` with the typed code, no retry storm.
 *
 * Nothing here decides who may watch. The BFF and Session Core did, at every attach.
 */

export const MAX_LIVE_ATTACHES = 3
const MAX_FAILED_ATTACHES = 5
const BASE_DELAY_MS = 800
const MAX_DELAY_MS = 15_000

export type SpaceRunStream = (
  spaceRef: string,
  runId: string,
  handlers: SpaceRunLiveHandlers,
  options: SpaceRunLiveOptions,
) => Promise<void>

export type SpaceRunStoreOptions = {
  readonly stream?: SpaceRunStream
  readonly maxAttaches?: number
  /** 0..1, replaceable for tests. */
  readonly random?: () => number
}

export interface SpaceRunStore {
  /** The current view of a run, reactive. Undefined for a run nobody has attached. */
  view(runId: string): SpaceRunView | undefined
  /** Every attached run, reactive. */
  views(): readonly SpaceRunView[]
  /** Start (or join) the live view of a run. Returns the release for this viewer. */
  attach(spaceRef: string, runId: string): () => void
  /** Close every stream and forget every view. */
  dispose(): void
}

type Entry = {
  readonly spaceRef: string
  readonly runId: string
  refs: number
  active: boolean
  terminal: boolean
  failures: number
  gotSnapshot: boolean
  retriedEarlyExpiry: boolean
  controller: AbortController | undefined
  timer: ReturnType<typeof setTimeout> | undefined
  lastEventId: string | undefined
}

/** Final answers from the route: nothing to retry. */
const FINAL_CODES = new Set([
  'feature_unavailable',
  'zdr_feature_not_supported',
  'space_read_not_authorized',
  'active_membership_required',
  'run_not_found',
  'space_not_found',
  'too_many_live_views',
  'invalid_space',
  'invalid_run',
])

export function createSpaceRunStore(options: SpaceRunStoreOptions = {}): SpaceRunStore {
  const stream = options.stream ?? streamSpaceRunLive
  const maxAttaches = options.maxAttaches ?? MAX_LIVE_ATTACHES
  const random = options.random ?? Math.random
  const entries = new Map<string, Entry>()
  const [map, setMap] = createSignal<ReadonlyMap<string, SpaceRunView>>(new Map())

  const patch = (runId: string, change: (view: SpaceRunView) => SpaceRunView) => {
    setMap((prev) => {
      const current = prev.get(runId)
      if (!current) return prev
      const next = change(current)
      if (next === current) return prev
      return new Map(prev).set(runId, next)
    })
  }

  const drop = (runId: string) => {
    setMap((prev) => {
      if (!prev.has(runId)) return prev
      const next = new Map(prev)
      next.delete(runId)
      return next
    })
  }

  const activeCount = () => {
    let total = 0
    for (const entry of entries.values()) if (entry.active) total += 1
    return total
  }

  const delayFor = (failures: number) => {
    const base = Math.min(MAX_DELAY_MS, BASE_DELAY_MS * 2 ** Math.max(0, failures - 1))
    return Math.round(base * (0.6 + random() * 0.8))
  }

  const markUnavailable = (entry: Entry, code: string | null) => {
    entry.terminal = true
    patch(entry.runId, (view) => ({ ...view, status: 'unavailable', unavailableCode: code }))
  }

  function pump() {
    for (const entry of entries.values()) {
      if (entry.active || entry.terminal || entry.timer !== undefined) continue
      if (activeCount() >= maxAttaches) {
        patch(entry.runId, (view) => (view.status === 'connecting' ? { ...view, status: 'waiting' } : view))
        continue
      }
      start(entry)
    }
  }

  function scheduleRetry(entry: Entry, delayMs: number) {
    if (entry.timer !== undefined) return
    entry.timer = setTimeout(() => {
      entry.timer = undefined
      if (entries.get(entry.runId) !== entry) return
      pump()
    }, delayMs)
  }

  function start(entry: Entry) {
    const controller = new AbortController()
    entry.controller = controller
    entry.active = true
    const onFrame = (frame: RoomRunFrame, sseId: string | undefined) => {
      if (entries.get(entry.runId) !== entry || controller.signal.aborted) return
      if (sseId) entry.lastEventId = sseId
      if (frame.type === 'snapshot') {
        entry.gotSnapshot = true
        entry.failures = 0
      }
      if (frame.type === 'finished') entry.terminal = true
      if (frame.type === 'error' && !frame.retryable) entry.terminal = true
      patch(entry.runId, (view) => applyRoomRunFrame(view, frame))
      const view = map().get(entry.runId)
      // A gap in the text, or an authorization that is about to end, is healed by attaching again;
      // closing this stream hands over to `onEnd`, which reconnects.
      if (view && (view.needsReattach || view.reauthRequired) && !entry.terminal) {
        entry.lastEventId = undefined
        controller.abort()
      }
    }
    const onEnd = (end: SpaceRunLiveEnd) => {
      entry.active = false
      entry.controller = undefined
      if (entries.get(entry.runId) !== entry) return
      if (entry.terminal) {
        pump()
        return
      }
      if (end.kind === 'failed') {
        const code = end.code ?? ''
        // The run pointer is written just after the turn starts: a 410 before any snapshot is retried once.
        if (end.status === 410 && !entry.gotSnapshot && !entry.retriedEarlyExpiry) {
          entry.retriedEarlyExpiry = true
          scheduleRetry(entry, delayFor(1))
          pump()
          return
        }
        if (end.status === 410) return finish(entry, code || 'run_stream_expired')
        if (FINAL_CODES.has(code) || (end.status >= 400 && end.status < 500 && end.status !== 408 && end.status !== 429)) {
          return finish(entry, code || null)
        }
        entry.failures += 1
        if (entry.failures >= MAX_FAILED_ATTACHES) return finish(entry, code || 'run_live_unavailable')
        scheduleRetry(entry, delayFor(entry.failures))
        pump()
        return
      }
      // Closed without a `finished` frame: the attach ended (authorization window, proxy timeout). Attach again.
      const reattached = map().get(entry.runId)?.reauthRequired || map().get(entry.runId)?.needsReattach
      if (end.kind === 'closed' || reattached) {
        entry.failures = reattached ? 0 : entry.failures + 1
        if (entry.failures >= MAX_FAILED_ATTACHES) return finish(entry, 'run_live_unavailable')
        patch(entry.runId, (view) => ({ ...view, needsReattach: false, reauthRequired: false }))
        scheduleRetry(entry, reattached ? Math.round(150 + random() * 450) : delayFor(entry.failures))
      }
      pump()
    }
    void stream(entry.spaceRef, entry.runId, { onFrame, onEnd }, {
      signal: controller.signal,
      ...(entry.lastEventId ? { lastEventId: entry.lastEventId } : {}),
    })
  }

  function finish(entry: Entry, code: string | null) {
    markUnavailable(entry, code)
    pump()
  }

  function release(entry: Entry) {
    entry.refs -= 1
    if (entry.refs > 0) return
    entries.delete(entry.runId)
    if (entry.timer !== undefined) clearTimeout(entry.timer)
    entry.controller?.abort()
    drop(entry.runId)
    pump()
  }

  return {
    view: (runId) => map().get(runId),
    views: () => [...map().values()],
    attach(spaceRef, runId) {
      let entry = entries.get(runId)
      if (!entry) {
        entry = {
          spaceRef,
          runId,
          refs: 0,
          active: false,
          terminal: false,
          failures: 0,
          gotSnapshot: false,
          retriedEarlyExpiry: false,
          controller: undefined,
          timer: undefined,
          lastEventId: undefined,
        }
        entries.set(runId, entry)
        setMap((prev) => new Map(prev).set(runId, emptySpaceRunView(runId)))
      }
      entry.refs += 1
      const joined = entry
      pump()
      let released = false
      return () => {
        if (released) return
        released = true
        release(joined)
      }
    },
    dispose() {
      for (const entry of entries.values()) {
        if (entry.timer !== undefined) clearTimeout(entry.timer)
        entry.controller?.abort()
      }
      entries.clear()
      setMap(new Map())
    },
  }
}

/** The store every room surface shares. */
export const spaceRunStore: SpaceRunStore = createSpaceRunStore()

/**
 * Keep the live view of these runs attached for as long as the calling component lives. Call it in a
 * component body; it diffs the wanted ids, attaches the new ones, releases the ones that went away and
 * releases everything on cleanup. With `enabled()` false nothing is attached (flag off, no live runs).
 */
export function trackLiveRuns(
  spaceRef: () => string,
  runIds: () => readonly string[],
  enabled: () => boolean,
  store: SpaceRunStore = spaceRunStore,
): void {
  const held = new Map<string, { readonly spaceRef: string; readonly release: () => void }>()
  const releaseAll = () => {
    for (const entry of held.values()) entry.release()
    held.clear()
  }
  createEffect(
    () => ({ ref: spaceRef(), ids: enabled() ? [...new Set(runIds().filter((id) => id.trim()))] : [] }),
    ({ ref, ids }) => {
      for (const [id, entry] of [...held]) {
        if (entry.spaceRef !== ref || !ids.includes(id)) {
          entry.release()
          held.delete(id)
        }
      }
      for (const id of ids) {
        if (!held.has(id)) held.set(id, { spaceRef: ref, release: store.attach(ref, id) })
      }
    },
  )
  onCleanup(releaseAll)
}
