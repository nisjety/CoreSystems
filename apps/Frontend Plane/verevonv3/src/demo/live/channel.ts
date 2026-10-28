/**
 * Live SSE channels of the demo engine. A channel is one `text/event-stream`
 * response. Frames are written now or after a delay. The stream closes when
 * asked, after an idle timeout, or when the visitor aborts it. Timers belong to
 * the channel, so closing it cancels everything it still had to send.
 *
 * The engine never talks to a network: the response is returned to the
 * in-page fetch interceptor (src/demo/install.ts), which hands it to the app.
 */
import { encodeFrame } from './frames'
import type { StreamFrame } from './scenario-types'

/** Whatever the scheduler hands back to cancel a timer. */
export type TimerHandle = unknown

/** Time and timers. The browser implementation is `createBrowserScheduler`; tests inject a manual one. */
export type Scheduler = {
  /** Monotonic milliseconds. */
  now(): number
  setTimeout(callback: () => void, ms: number): TimerHandle
  clearTimeout(handle: TimerHandle): void
}

export type LiveChannel = {
  readonly label: string
  readonly isOpen: boolean
  /** The response a route returns. Undefined for channels that are not streams (tests). */
  readonly response: Response | undefined
  /** Write one frame now. Dropped once the channel is closed. */
  send(item: StreamFrame): void
  /** Write one frame after `delayMs`. Dropped if the channel closes first. */
  sendAfter(delayMs: number, item: StreamFrame): void
  /** Close after `delayMs`; frames scheduled before that still go out. */
  closeAfter(delayMs: number): void
  /** Close now, cancelling every timer the channel holds. Idempotent. */
  close(): void
  /** Runs once when the channel closes (at once if it already has). */
  onClose(listener: () => void): void
}

export type ChannelOptions = {
  /** Close after this much time without a write. Pending approvals keep a turn open for a long while. */
  idleCloseMs?: number
}

export type ChannelFactory = (label: string, options?: ChannelOptions) => LiveChannel

/** The scheduler of the page: `performance.now` and the window's timers. */
export function createBrowserScheduler(): Scheduler {
  return {
    now: () => performance.now(),
    setTimeout: (callback, ms) => window.setTimeout(callback, ms),
    clearTimeout: (handle) => window.clearTimeout(handle as number),
  }
}

/** A factory of SSE channels whose response is a real `ReadableStream`. */
export function createStreamChannelFactory(scheduler: Scheduler): ChannelFactory {
  return (label, options = {}) => {
    const encoder = new TextEncoder()
    const timers = new Set<TimerHandle>()
    const listeners: Array<() => void> = []
    let controller: ReadableStreamDefaultController<Uint8Array> | undefined
    let open = true
    let idle: TimerHandle | undefined
    let closeTimer: TimerHandle | undefined

    /** Marks the channel closed and cancels every timer it holds. */
    const finish = () => {
      if (!open) return
      open = false
      for (const timer of timers) scheduler.clearTimeout(timer)
      timers.clear()
      if (idle !== undefined) scheduler.clearTimeout(idle)
      idle = undefined
      for (const listener of listeners.splice(0)) listener()
    }

    const close = () => {
      if (!open) return
      const target = controller
      finish()
      try {
        target?.close()
      } catch {
        // The reader already went away (the visitor aborted the request).
      }
    }

    const touch = () => {
      if (options.idleCloseMs === undefined) return
      if (idle !== undefined) scheduler.clearTimeout(idle)
      idle = scheduler.setTimeout(close, options.idleCloseMs)
    }

    const body = new ReadableStream<Uint8Array>({
      start(stream) {
        controller = stream
      },
      // The visitor stopped reading (Stopp svar, navigation): nothing more is owed.
      cancel() {
        finish()
      },
    })

    const channel: LiveChannel = {
      label,
      get isOpen() {
        return open
      },
      response: new Response(body, {
        status: 200,
        headers: { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache' },
      }),
      send(item) {
        if (!open || !controller) return
        try {
          controller.enqueue(encoder.encode(encodeFrame(item)))
        } catch {
          finish()
          return
        }
        touch()
      },
      sendAfter(delayMs, item) {
        if (!open) return
        const handle = scheduler.setTimeout(() => {
          timers.delete(handle)
          channel.send(item)
        }, Math.max(0, delayMs))
        timers.add(handle)
      },
      closeAfter(delayMs) {
        if (!open) return
        // Re-arming replaces the earlier close: the latest request decides when the stream ends.
        if (closeTimer !== undefined) {
          scheduler.clearTimeout(closeTimer)
          timers.delete(closeTimer)
        }
        const handle = scheduler.setTimeout(() => {
          timers.delete(handle)
          closeTimer = undefined
          close()
        }, Math.max(0, delayMs))
        closeTimer = handle
        timers.add(handle)
      },
      close,
      onClose(listener) {
        if (open) listeners.push(listener)
        else listener()
      },
    }

    touch()
    return channel
  }
}
