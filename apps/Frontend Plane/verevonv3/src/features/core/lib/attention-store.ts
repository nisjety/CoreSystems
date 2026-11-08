import { announceAttentionChanged, type AttentionItem } from '@/shared/api/attention-client'
import {
  ATTENTION_HEARTBEAT_MS,
  AttentionFrameGate,
  openAttentionStream,
  type AttentionFrame,
} from '@/shared/api/attention-stream'
import { isWaitingState, type AttentionState } from '@/shared/attention/attention-state'

/** The navbar refresh while the attention stream is live: a long backstop for what the navbar shows besides attention. */
export const NAVBAR_BACKSTOP_MS = 300_000
const LEGACY_VISIBLE_MS = 60_000
const LEGACY_HIDDEN_MS = 180_000
/** A stream that has sent nothing for two heartbeats and a margin is unhealthy. */
const QUIET_MS = ATTENTION_HEARTBEAT_MS * 2 + 5_000
const RETRY_MIN_MS = 1_000
const RETRY_MAX_MS = 60_000
const LOCK_NAME = 'verevon-attention-stream'
const CHANNEL_NAME = 'verevon-attention-stream'

export type AttentionStreamHealth = 'connecting' | 'live' | 'unhealthy' | 'stopped'

/**
 * How often the navbar refreshes. With the stream off it keeps the legacy cadence. With the stream live it is
 * a backstop, because the stream carries the attention changes. While the stream is down the legacy cadence
 * comes back, so attention is never slower than it was before the stream existed.
 */
export function navbarRefreshDelayMs(input: { streamSupported: boolean; streamLive: boolean; hidden: boolean }): number {
  if (input.streamSupported && input.streamLive) return NAVBAR_BACKSTOP_MS
  return input.hidden ? LEGACY_HIDDEN_MS : LEGACY_VISIBLE_MS
}

export type AttentionStore = {
  /** Starts the stream. A stopped store is not started again; the shell makes a new one. */
  start(): void
  stop(): void
  health(): AttentionStreamHealth
  subscribe(listener: (health: AttentionStreamHealth) => void): () => void
}

export type AttentionStoreOptions = {
  /** Called when a thread starts waiting on the person: an approval, a proposed plan or a question. */
  onWaiting?: (item: AttentionItem) => void
  /** Opens one stream. Defaults to the attention stream of this origin. */
  openStream?: (options: {
    signal: AbortSignal
    onFrame: (frame: AttentionFrame) => void
    onError?: (error: unknown) => void
  }) => Promise<void>
  /** The lock that makes one tab per browser profile the holder of the stream. Defaults to `navigator.locks`. */
  locks?: LockRequester | null
  /** The channel the holder shares its frames on with the other tabs. Defaults to a `BroadcastChannel`. */
  channel?: FrameChannel | null
}

/** The part of `navigator.locks` the store uses. */
export type LockRequester = {
  request(name: string, options: { signal?: AbortSignal }, callback: () => Promise<void>): Promise<unknown>
}

/** The part of a `BroadcastChannel` the store uses. */
export type FrameChannel = {
  postMessage(message: unknown): void
  close(): void
  onmessage: ((event: MessageEvent) => void) | null
}

/**
 * The live attention view of one browser profile. One tab holds the stream through a lock and shares its
 * frames with the others, so a profile opens one connection however many tabs it has. Each tab applies the
 * frames to its own gate, so a stale or out-of-order frame is dropped wherever it arrives.
 */
export function createAttentionStore(options: AttentionStoreOptions = {}): AttentionStore {
  const openStream = options.openStream ?? openAttentionStream
  const locks = options.locks === undefined ? browserLocks() : options.locks
  // Frames only need sharing where a lock keeps the other tabs from streaming on their own.
  const channel = locks === null ? null : options.channel === undefined ? browserChannel() : options.channel
  const gate = new AttentionFrameGate()
  const listeners = new Set<(health: AttentionStreamHealth) => void>()
  const waiting = new Map<string, AttentionState | undefined>()
  let health: AttentionStreamHealth = 'connecting'
  let synced = false
  let run: AbortController | null = null
  let open: AbortController | null = null
  let quietTimer: ReturnType<typeof setTimeout> | undefined

  const setHealth = (next: AttentionStreamHealth) => {
    if (next === health) return
    health = next
    for (const listener of listeners) listener(next)
  }

  const armQuietTimer = () => {
    clearTimeout(quietTimer)
    quietTimer = setTimeout(() => {
      setHealth('unhealthy')
      open?.abort()
    }, QUIET_MS)
  }

  const apply = (frame: AttentionFrame) => {
    if (frame.kind === 'snapshot') {
      // A snapshot replaces the view, so the threads it already lists are not announced again.
      waiting.clear()
      for (const item of frame.items) waiting.set(item.threadId, item.state)
      synced = true
      announceAttentionChanged()
    } else if (frame.kind === 'changed') {
      const { item } = frame
      const before = waiting.get(item.threadId)
      if (item.state === 'idle') waiting.delete(item.threadId)
      else waiting.set(item.threadId, item.state)
      if (isWaitingState(item.state) && !isWaitingState(before)) options.onWaiting?.(item)
      announceAttentionChanged()
    } else if (frame.kind === 'resync') {
      // The server could not read the rows; what it sends next is a fresh snapshot.
      synced = false
    }
    setHealth(synced ? 'live' : 'connecting')
  }

  const receive = (frame: AttentionFrame): 'apply' | 'stale' | 'reconnect' => {
    const verdict = gate.accept(frame)
    if (verdict === 'reconnect') {
      open?.abort()
    } else if (verdict === 'apply') {
      armQuietTimer()
      apply(frame)
    }
    return verdict
  }

  // Reads the stream until the store stops, reconnecting with a backoff. Only the lock holder runs this.
  const holdStream = async (signal: AbortSignal) => {
    let retryMs = RETRY_MIN_MS
    while (!signal.aborted) {
      const attempt = new AbortController()
      const endAttempt = () => attempt.abort()
      signal.addEventListener('abort', endAttempt, { once: true })
      open = attempt
      armQuietTimer()
      setHealth('connecting')
      await openStream({
        signal: attempt.signal,
        onFrame: (frame) => {
          if (receive(frame) !== 'apply') return
          retryMs = RETRY_MIN_MS
          channel?.postMessage({ type: 'frame', frame })
        },
      })
      signal.removeEventListener('abort', endAttempt)
      clearTimeout(quietTimer)
      open = null
      if (signal.aborted) return
      setHealth('unhealthy')
      await sleep(retryMs, signal)
      retryMs = Math.min(retryMs * 2, RETRY_MAX_MS)
    }
  }

  return {
    start() {
      if (run) return
      const controller = new AbortController()
      run = controller
      if (channel) {
        channel.onmessage = (event: MessageEvent) => {
          const message = event.data as { type?: unknown; frame?: AttentionFrame } | null
          if (message?.type === 'frame' && message.frame) receive(message.frame)
        }
      }
      if (locks) {
        locks
          .request(LOCK_NAME, { signal: controller.signal }, () => holdStream(controller.signal))
          .catch(() => undefined)
      } else {
        void holdStream(controller.signal)
      }
    },
    stop() {
      run?.abort()
      run = null
      clearTimeout(quietTimer)
      if (channel) {
        channel.onmessage = null
        channel.close()
      }
      setHealth('stopped')
    },
    health: () => health,
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
  }
}

function browserLocks(): LockRequester | null {
  return typeof navigator !== 'undefined' && navigator.locks ? (navigator.locks as LockRequester) : null
}

function browserChannel(): FrameChannel | null {
  return typeof BroadcastChannel === 'function' ? new BroadcastChannel(CHANNEL_NAME) : null
}

/** Waits for `ms`, or until `signal` aborts, whichever comes first. */
function sleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const finish = () => {
      clearTimeout(timer)
      signal.removeEventListener('abort', finish)
      resolve()
    }
    const timer = setTimeout(finish, ms)
    signal.addEventListener('abort', finish, { once: true })
  })
}
