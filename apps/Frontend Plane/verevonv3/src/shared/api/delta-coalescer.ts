/**
 * Frame-batched delivery of streamed text (WS09 S4).
 *
 * A stream can carry several `chunk` frames inside one display frame, and each
 * one used to re-render the answer on its own. The coalescer joins the text of
 * consecutive deltas and emits it once per animation frame.
 *
 * It never reorders. A run only grows while the next delta has the same kind and
 * request id; anything else emits the run first. The dispatcher also flushes
 * before every frame that is not a delta, so a tool call, a citation or a
 * terminal event still reaches its handler after the text that came before it.
 *
 * Text waits here for at most one frame. A 100 ms timer is armed beside the
 * frame because a hidden tab runs no animation frames. The abort listener is
 * synchronous: text already received is in the transcript before a Stop, a
 * thread switch or an unmount marks the turn as ended.
 */

export type DeltaKind = 'message' | 'reasoning'

export type DeltaRun = {
  kind: DeltaKind
  requestId: string | undefined
  text: string
}

export type DeltaCoalescer = {
  /** Adds a delta. A different kind or request id emits the pending run first. */
  push: (kind: DeltaKind, requestId: string | undefined, text: string) => void
  /** Emits the pending run now and cancels the frame that would have emitted it. */
  flush: () => void
  /** Emits what is pending and stops listening to the abort signal. */
  close: () => void
}

const HIDDEN_TAB_FLUSH_MS = 100

/** A coalescer that may still hold text: created, and not yet closed or ended by its abort signal. */
type LiveCoalescer = {
  flush: () => void
  /** Whether a run is waiting for its frame, so its text is not in the transcript yet. */
  hasPending: () => boolean
}

/**
 * The coalescers that may still hold streamed text (F-f01-19). The chat controller flushes them before it
 * writes the transcript snapshot on hide, pagehide and unmount, so that snapshot carries the text.
 */
const liveCoalescers = new Set<LiveCoalescer>()

/**
 * Emits the text every live coalescer still holds, in creation order. A no-op when none is live.
 *
 * An emit that throws is reported as an uncaught error, as a throw from a frame callback is, and does not stop the
 * others: this runs on hide, pagehide and unmount, where the snapshot that follows must still be written.
 */
export function flushAllDeltaCoalescers(): void {
  // A copy: an emit can close a coalescer, which must not change the set under this loop.
  for (const live of [...liveCoalescers]) {
    try {
      live.flush()
    } catch (error) {
      queueMicrotask(() => {
        throw error
      })
    }
  }
}

/** Whether a live coalescer holds text the transcript does not have yet (F-f01-19). */
export function hasPendingDeltaText(): boolean {
  for (const live of liveCoalescers) if (live.hasPending()) return true
  return false
}

export function createDeltaCoalescer(
  emit: (run: DeltaRun) => void,
  signal?: AbortSignal,
): DeltaCoalescer {
  let pending: DeltaRun | undefined
  let frame: number | undefined
  let timer: ReturnType<typeof setTimeout> | undefined

  const flush = (): void => {
    if (frame !== undefined) cancelAnimationFrame(frame)
    if (timer !== undefined) clearTimeout(timer)
    frame = undefined
    timer = undefined
    const run = pending
    if (!run) return
    // Cleared before emitting, so a handler that throws is not handed its text again.
    pending = undefined
    emit(run)
  }

  const schedule = (): void => {
    if (frame === undefined && typeof requestAnimationFrame === 'function') {
      frame = requestAnimationFrame(flush)
    }
    if (timer === undefined) timer = setTimeout(flush, HIDDEN_TAB_FLUSH_MS)
  }

  const live: LiveCoalescer = { flush, hasPending: () => pending !== undefined }
  liveCoalescers.add(live)

  // Synchronous, so text already received is in the transcript before a Stop, a thread switch or an unmount marks
  // the turn as ended. Once it has flushed, the coalescer is no longer live, even if an emit threw.
  const onAbort = (): void => {
    try {
      flush()
    } finally {
      liveCoalescers.delete(live)
    }
  }
  signal?.addEventListener('abort', onAbort)

  return {
    push(kind, requestId, text) {
      if (pending && (pending.kind !== kind || pending.requestId !== requestId)) flush()
      if (!pending) pending = { kind, requestId, text: '' }
      pending.text += text
      schedule()
    },
    flush,
    close() {
      try {
        flush()
      } finally {
        liveCoalescers.delete(live)
        signal?.removeEventListener('abort', onAbort)
      }
    },
  }
}
