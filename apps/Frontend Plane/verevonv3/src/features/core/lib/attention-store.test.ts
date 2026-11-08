import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AttentionItem } from '@/shared/api/attention-client'
import type { AttentionFrame } from '@/shared/api/attention-stream'
import type { AttentionState } from '@/shared/attention/attention-state'
import {
  createAttentionStore,
  NAVBAR_BACKSTOP_MS,
  navbarRefreshDelayMs,
  type AttentionStreamHealth,
  type FrameChannel,
} from './attention-store'

/** The silence after which a live stream counts as unhealthy: two heartbeats and a margin. */
const QUIET_MS = 45_000
const counts = { needsYou: 0, working: 0, unseen: 0 }

type Opened = { signal: AbortSignal; onFrame: (frame: AttentionFrame) => void }

/** A stream that stays open until its signal aborts, so a test drives it by hand. */
function fakeOpener() {
  const opened: Opened[] = []
  const openStream = vi.fn(
    (options: Opened) =>
      new Promise<void>((resolve) => {
        opened.push(options)
        options.signal.addEventListener('abort', () => resolve(), { once: true })
      }),
  )
  return { openStream, opened }
}

/** The stream the store opened at `index`, in the order it opened them. */
function streamAt(opened: Opened[], index = 0): Opened {
  const stream = opened[index]
  if (!stream) throw new Error(`the store has not opened stream ${index}`)
  return stream
}

/** Channels that deliver what one posts to every other channel of the same bus. */
function fakeBus() {
  const channels: Array<{ handler: ((event: MessageEvent) => void) | null }> = []
  return {
    open(): FrameChannel {
      const self: { handler: ((event: MessageEvent) => void) | null } = { handler: null }
      channels.push(self)
      return {
        postMessage(message: unknown) {
          for (const other of channels) {
            if (other !== self) other.handler?.({ data: message } as MessageEvent)
          }
        },
        close() {},
        get onmessage() {
          return self.handler
        },
        set onmessage(handler) {
          self.handler = handler
        },
      }
    },
  }
}

/** A lock that one caller holds at a time; the others wait, as `navigator.locks` makes them. */
function fakeLocks() {
  let held = false
  return {
    request: vi.fn((_name: string, _options: { signal?: AbortSignal }, callback: () => Promise<void>) => {
      if (held) return new Promise<void>(() => undefined)
      held = true
      return callback().finally(() => {
        held = false
      })
    }),
  }
}

const item = (threadId: string, state: AttentionState): AttentionItem => ({ threadId, state, unseen: false })
const snapshot = (seq: number, items: AttentionItem[] = []): AttentionFrame => ({
  kind: 'snapshot',
  epoch: 'epoch-1',
  seq,
  items,
  counts,
})
const changed = (seq: number, next: AttentionItem): AttentionFrame => ({
  kind: 'changed',
  epoch: 'epoch-1',
  seq,
  item: next,
  counts,
})
const heartbeat = (seq: number): AttentionFrame => ({ kind: 'heartbeat', epoch: 'epoch-1', seq })

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('navbarRefreshDelayMs', () => {
  it('keeps the legacy cadence while the stream is not served', () => {
    expect(navbarRefreshDelayMs({ streamSupported: false, streamLive: false, hidden: false })).toBe(60_000)
    expect(navbarRefreshDelayMs({ streamSupported: false, streamLive: false, hidden: true })).toBe(180_000)
  })

  it('backs the navbar off to the long fallback only while a served stream is live', () => {
    expect(navbarRefreshDelayMs({ streamSupported: true, streamLive: true, hidden: false })).toBe(NAVBAR_BACKSTOP_MS)
    expect(navbarRefreshDelayMs({ streamSupported: true, streamLive: false, hidden: false })).toBe(60_000)
    expect(navbarRefreshDelayMs({ streamSupported: true, streamLive: false, hidden: true })).toBe(180_000)
  })
})

describe('createAttentionStore', () => {
  it('applies the snapshot, reports the view live, and reports stopped when it stops', () => {
    const { openStream, opened } = fakeOpener()
    const store = createAttentionStore({ openStream, locks: null, channel: null })
    const seen: AttentionStreamHealth[] = []
    store.subscribe((health) => seen.push(health))

    store.start()
    streamAt(opened).onFrame(snapshot(1))
    expect(store.health()).toBe('live')

    store.stop()
    expect(store.health()).toBe('stopped')
    expect(seen).toEqual(['live', 'stopped'])
  })

  it('announces a thread only when it starts to wait on the person, and never a thread a snapshot lists', () => {
    const { openStream, opened } = fakeOpener()
    const onWaiting = vi.fn()
    const store = createAttentionStore({ openStream, onWaiting, locks: null, channel: null })
    store.start()
    const deliver = (frame: AttentionFrame) => streamAt(opened).onFrame(frame)

    deliver(snapshot(1, [item('thread-1', 'needs_approval')]))
    deliver(changed(2, item('thread-1', 'needs_approval')))
    deliver(changed(3, item('thread-2', 'plan_ready')))
    deliver(changed(4, item('thread-2', 'working')))
    deliver(changed(5, item('thread-2', 'needs_input')))

    expect(onWaiting.mock.calls.map(([listed]) => listed.threadId)).toEqual(['thread-2', 'thread-2'])
    store.stop()
  })

  it('drops a stale frame, and reads the stream again after a gap', async () => {
    const { openStream, opened } = fakeOpener()
    const store = createAttentionStore({ openStream, locks: null, channel: null })
    store.start()
    const deliver = (frame: AttentionFrame) => streamAt(opened).onFrame(frame)
    deliver(snapshot(1))
    deliver(changed(2, item('thread-1', 'working')))

    deliver(changed(2, item('thread-1', 'completed')))
    expect(openStream).toHaveBeenCalledTimes(1)

    deliver(changed(4, item('thread-1', 'failed')))
    expect(streamAt(opened).signal.aborted).toBe(true)
    await vi.advanceTimersByTimeAsync(1_000)
    expect(openStream).toHaveBeenCalledTimes(2)
    store.stop()
  })

  it('turns unhealthy when the stream goes quiet, and stays live while heartbeats arrive', async () => {
    const { openStream, opened } = fakeOpener()
    const store = createAttentionStore({ openStream, locks: null, channel: null })
    store.start()
    streamAt(opened).onFrame(snapshot(1))

    await vi.advanceTimersByTimeAsync(20_000)
    streamAt(opened).onFrame(heartbeat(2))
    await vi.advanceTimersByTimeAsync(20_000)
    streamAt(opened).onFrame(heartbeat(3))
    expect(store.health()).toBe('live')

    await vi.advanceTimersByTimeAsync(QUIET_MS)
    expect(store.health()).toBe('unhealthy')
    expect(streamAt(opened).signal.aborted).toBe(true)
    store.stop()
  })

  it('waits for the next snapshot after a resync before it reports the view live again', () => {
    const { openStream, opened } = fakeOpener()
    const store = createAttentionStore({ openStream, locks: null, channel: null })
    store.start()
    const deliver = (frame: AttentionFrame) => streamAt(opened).onFrame(frame)
    deliver(snapshot(1))

    deliver({ kind: 'resync', epoch: 'epoch-1', seq: 2 })
    expect(store.health()).toBe('connecting')

    deliver(snapshot(3))
    expect(store.health()).toBe('live')
    store.stop()
  })

  it('lets one tab hold the stream and feeds the other tabs from its frames', () => {
    const { openStream, opened } = fakeOpener()
    const bus = fakeBus()
    const locks = fakeLocks()
    const first = createAttentionStore({ openStream, locks, channel: bus.open() })
    const second = createAttentionStore({ openStream, locks, channel: bus.open() })

    first.start()
    second.start()
    expect(openStream).toHaveBeenCalledTimes(1)

    streamAt(opened).onFrame(snapshot(1, [item('thread-1', 'needs_approval')]))
    expect(second.health()).toBe('live')

    first.stop()
    second.stop()
  })
})
