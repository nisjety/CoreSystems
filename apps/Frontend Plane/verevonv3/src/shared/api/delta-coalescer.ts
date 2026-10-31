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

  signal?.addEventListener('abort', flush)

  return {
    push(kind, requestId, text) {
      if (pending && (pending.kind !== kind || pending.requestId !== requestId)) flush()
      if (!pending) pending = { kind, requestId, text: '' }
      pending.text += text
      schedule()
    },
    flush,
    close() {
      flush()
      signal?.removeEventListener('abort', flush)
    },
  }
}
