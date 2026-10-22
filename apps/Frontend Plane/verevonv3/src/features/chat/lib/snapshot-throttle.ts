/**
 * Cadence for the local transcript snapshot while an answer streams.
 *
 * WS09 S3. The controller's persistence effect re-runs on every token and used
 * to rewrite the whole transcript to localStorage AND sessionStorage each time,
 * so the bytes written grew quadratically with the answer. This writer sits
 * between that effect and `writeThreadSnapshot`: it delays CALLS, nothing else.
 * It never reads or stores content, so every guard that decides whether a
 * snapshot may be written at all (temporary thread, foreign-origin thread, the
 * org's retention posture) still runs inside `write`, at the moment the write
 * actually happens. A pending write for a thread that turned temporary, or whose
 * org turned ZDR, before the timer fired is therefore dropped by those guards,
 * never flushed past them.
 *
 * Shape: the first call after a quiet period writes at once (so a one-shot
 * change is never delayed); calls inside the following `intervalMs` collapse into
 * ONE trailing write when the window closes. `flush()` writes the pending call
 * now and is what the terminal, `visibilitychange`, `pagehide`, thread-switch
 * and cleanup triggers call. The cadence is driven by timers, not by reading
 * the clock, so it is exact under fake timers.
 */

/** One snapshot per second while streaming; `0` writes on every call, as before. */
export const DEFAULT_SNAPSHOT_THROTTLE_MS = 1000

/** A longer window would risk more than a few seconds of local-only partial text. */
const MAX_SNAPSHOT_THROTTLE_MS = 60_000

/**
 * `VITE_CHAT_SNAPSHOT_THROTTLE_MS`: whole milliseconds only. Anything else (unset,
 * empty, negative, fractional, text) keeps the default rather than guessing.
 */
export function parseSnapshotThrottleMs(raw: string | undefined): number {
  if (raw === undefined || !/^\d{1,6}$/.test(raw)) return DEFAULT_SNAPSHOT_THROTTLE_MS
  return Math.min(Number(raw), MAX_SNAPSHOT_THROTTLE_MS)
}

export type SnapshotWriter<T> = {
  /** Record the latest call; writes now, or at the end of the current window. */
  schedule: (payload: T) => void
  /** Write the pending call now (no-op when none), regardless of the window. */
  flush: () => void
  /** Drop the pending call without writing it. */
  cancel: () => void
  /** Whether a call is waiting for the window to close. */
  hasPending: () => boolean
  /** Stop the timer. A pending call is dropped; flush first to keep it. */
  dispose: () => void
}

export function createSnapshotWriter<T>(
  write: (payload: T) => void,
  options: { intervalMs: number },
): SnapshotWriter<T> {
  const intervalMs = options.intervalMs
  let timer: ReturnType<typeof setTimeout> | undefined
  let pending: { payload: T } | null = null

  const closeWindow = () => {
    timer = undefined
    const next = pending
    pending = null
    if (next) writeAndOpenWindow(next.payload)
  }
  const writeAndOpenWindow = (payload: T) => {
    // The window opens before the write so a throwing write cannot leave the
    // writer unthrottled for the rest of the stream.
    if (timer !== undefined) clearTimeout(timer)
    timer = setTimeout(closeWindow, intervalMs)
    write(payload)
  }

  return {
    schedule(payload) {
      if (intervalMs <= 0) {
        write(payload)
        return
      }
      if (timer === undefined) {
        writeAndOpenWindow(payload)
        return
      }
      pending = { payload }
    },
    flush() {
      const next = pending
      pending = null
      if (!next) return
      if (intervalMs <= 0) {
        write(next.payload)
        return
      }
      writeAndOpenWindow(next.payload)
    },
    cancel() {
      pending = null
    },
    hasPending: () => pending !== null,
    dispose() {
      pending = null
      if (timer !== undefined) clearTimeout(timer)
      timer = undefined
    },
  }
}

/**
 * Flush when the page is about to be hidden or unloaded: the last moment the
 * browser still lets a synchronous storage write through. Returns the unbind.
 */
export function flushOnPageLifecycle(
  writer: Pick<SnapshotWriter<unknown>, 'flush'>,
  target?: { document: Document; window: Window },
): () => void {
  const env = target ?? (typeof document === 'undefined' ? undefined : { document, window })
  if (!env) return () => {}
  const onVisibility = () => {
    if (env.document.visibilityState === 'hidden') writer.flush()
  }
  const onPageHide = () => writer.flush()
  env.document.addEventListener('visibilitychange', onVisibility)
  env.window.addEventListener('pagehide', onPageHide)
  return () => {
    env.document.removeEventListener('visibilitychange', onVisibility)
    env.window.removeEventListener('pagehide', onPageHide)
  }
}
