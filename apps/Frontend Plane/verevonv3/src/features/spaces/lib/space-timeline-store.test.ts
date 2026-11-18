import { flush } from 'solid-js'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/shared/api/http'
import type {
  SpaceSystemEntry,
  SpaceThread,
  SpaceTimeline,
  SpaceTimelineRequest,
  SpaceTranscriptLoad,
} from '@/shared/api/spaces-client'

const getSpaceTimeline = vi.fn()
const loadSpaceTranscript = vi.fn()

vi.mock('@/shared/api/spaces-client', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/shared/api/spaces-client')>()),
  getSpaceTimeline: (...args: unknown[]) => getSpaceTimeline(...args),
  loadSpaceTranscript: (...args: unknown[]) => loadSpaceTranscript(...args),
}))

import {
  createSpaceTimelineStore,
  TAIL_TURNS,
  TIMELINE_MAX_POSTS,
  TIMELINE_PAGE_LIMIT,
  TRANSCRIPT_CONCURRENCY,
  type SpaceTimelineStore,
} from './space-timeline-store'

const SPACE = 'room-a'
// Post N was last changed N minutes after a fixed start, so the sequence and the time always agree.
const stamp = (minute: number) => new Date(Date.UTC(2026, 10, 1, 9, 0, 0) + minute * 60_000).toISOString()
const cursorFor = (sequence: number) => `cur-${sequence}`
const sequenceFrom = (value: string) => Number(value.slice('cur-'.length))
const sequenceOf = (thread: SpaceThread) => thread.last_message_seq ?? 0
const idsOf = (items: readonly { readonly thread_id?: string; readonly id?: string }[]) =>
  items.map((item) => item.thread_id ?? item.id)

function firstOf<T>(items: readonly T[]): T {
  const [head] = items
  if (head === undefined) throw new Error('expected at least one item')
  return head
}

function post(sequence: number, overrides: Partial<SpaceThread> = {}): SpaceThread {
  return {
    thread_id: `thr-${sequence}`,
    space_id: 'space-room-a',
    title: `Post ${sequence}`,
    updated_at: stamp(sequence),
    last_message_seq: sequence,
    latest_run_status: 'completed',
    ...overrides,
  }
}

function systemRow(id: string, minute: number): SpaceSystemEntry {
  return { kind: 'system', id, at: stamp(minute), code: 'member_added' }
}

function timelineOf(
  threads: readonly SpaceThread[],
  page: Partial<SpaceTimeline['page']> = {},
  systemEntries: readonly SpaceSystemEntry[] = [],
): SpaceTimeline {
  return {
    page: { windowed: true, hasMore: false, nextBefore: null, nextAfter: null, newest: null, totalPosts: null, ...page },
    systemEntries,
    systemTruncated: false,
    threads,
    pinned: [],
    refreshed: [],
  }
}

// The room's posts, served the way the gateway pages them: keyset cursors on each post's sequence number,
// oldest first within a page. A test changes `room` to simulate new messages.
let room: SpaceThread[] = []

function serve(request: SpaceTimelineRequest): SpaceTimeline {
  const size = request.limit ?? TIMELINE_PAGE_LIMIT
  const all = [...room].sort((a, b) => sequenceOf(a) - sequenceOf(b))
  let slice: SpaceThread[]
  let hasMore = false
  if (request.before) {
    const cut = sequenceFrom(request.before)
    slice = all.filter((thread) => sequenceOf(thread) < cut).slice(-size)
  } else if (request.after) {
    const cut = sequenceFrom(request.after)
    const newer = all.filter((thread) => sequenceOf(thread) > cut)
    slice = newer.slice(0, size)
    hasMore = newer.length > slice.length
  } else {
    slice = all.slice(-size)
  }
  const oldest = slice[0]
  const newest = slice[slice.length - 1]
  const refreshed = (request.refresh ?? []).flatMap((id) => {
    const current = all.find((thread) => thread.thread_id === id)
    return current ? [{ ...current }] : []
  })
  return {
    page: {
      windowed: true,
      hasMore,
      nextBefore: oldest && all.some((thread) => sequenceOf(thread) < sequenceOf(oldest)) ? cursorFor(sequenceOf(oldest)) : null,
      nextAfter: null,
      newest: newest ? cursorFor(sequenceOf(newest)) : null,
      totalPosts: all.length,
    },
    systemEntries: [],
    systemTruncated: false,
    threads: slice.map((thread) => ({ ...thread })),
    pinned: [],
    refreshed,
  }
}

// A transcript read for one post: the post's first turn only, with the message id the store de-duplicates by.
function okRead(threadId: string): SpaceTranscriptLoad {
  const sequence = threadId.slice('thr-'.length)
  return { state: 'ok', threadId, turns: [{ role: 'user', content: `Post ${sequence}`, messageId: `msg-${sequence}-1` }] }
}

const turn = (messageId: string) => ({ role: 'user' as const, content: messageId, messageId })

beforeEach(() => {
  room = []
  getSpaceTimeline.mockReset()
  getSpaceTimeline.mockImplementation(async (_space: string, request: SpaceTimelineRequest) => serve(request))
  loadSpaceTranscript.mockReset()
  loadSpaceTranscript.mockImplementation(async (_space: string, threadId: string) => okRead(threadId))
})

let store: SpaceTimelineStore | undefined

function open(): SpaceTimelineStore {
  store = createSpaceTimelineStore(SPACE)
  return store
}

afterEach(() => {
  store?.dispose()
  store = undefined
})

// Lets the store's pending promise callbacks run, then applies Solid's pending signal writes.
async function settle(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
  flush()
}

// 500 posts on the server, read the way the room reads them: the newest page, then eight pages back. The
// eighth page takes the room past 400 posts, so the newest page is dropped whole.
async function readFullRoomPastTheCap(): Promise<SpaceTimelineStore> {
  room = Array.from({ length: 500 }, (_, i) => post(i + 1))
  const s = open()
  await s.loadFirst()
  for (let i = 0; i < 8; i += 1) await s.loadEarlier()
  flush()
  return s
}

function messageIdsOf(s: SpaceTimelineStore, threadId: string): (string | undefined)[] {
  return (s.transcript(threadId)?.turns ?? []).map((item) => item.messageId)
}

// WS15-S8: the room holds a window of keyset pages, never more than 400 posts, and never an offset.
describe('the windowed room timeline store (WS15-S8)', () => {
  it('loadFirst reads one page: the newest posts, oldest first, with a cursor for the older ones', async () => {
    room = Array.from({ length: 60 }, (_, i) => post(i + 1))
    const s = open()
    await s.loadFirst()
    flush()
    expect(getSpaceTimeline).toHaveBeenCalledTimes(1)
    expect(getSpaceTimeline).toHaveBeenCalledWith(SPACE, { limit: TIMELINE_PAGE_LIMIT })
    expect(idsOf(s.threads())).toEqual(Array.from({ length: 50 }, (_, i) => `thr-${i + 11}`))
    expect(s.hasEarlier()).toBe(true)
    expect(s.hasNewer()).toBe(false)
    expect(s.windowed()).toBe(true)
    expect(s.totalPosts()).toBe(60)
  })

  it('loadEarlier asks for the page before next_before, and skips the posts it already holds', async () => {
    getSpaceTimeline
      .mockResolvedValueOnce(timelineOf([post(3), post(4)], { nextBefore: cursorFor(3), newest: cursorFor(4) }))
      .mockResolvedValueOnce(
        timelineOf([post(1), post(2), post(3, { title: 'Moved' })], { nextBefore: null, newest: cursorFor(3) }),
      )
    const s = open()
    await s.loadFirst()
    await s.loadEarlier()
    flush()
    expect(getSpaceTimeline).toHaveBeenLastCalledWith(SPACE, { limit: TIMELINE_PAGE_LIMIT, before: cursorFor(3) })
    expect(idsOf(s.threads())).toEqual(['thr-1', 'thr-2', 'thr-3', 'thr-4'])
    // The copy already held is kept; the repeated one from the older page is not applied.
    expect(s.threads().find((thread) => thread.thread_id === 'thr-3')?.title).toBe('Post 3')
    expect(s.hasEarlier()).toBe(false)
  })

  it('refresh asks for the posts after the newest cursor, with the live and visible posts to re-read', async () => {
    room = [
      post(1),
      post(2, { latest_run_status: 'running' }),
      post(3, { latest_run_status: 'queued' }),
      post(4, { latest_run_status: 'awaiting_approval' }),
      post(5),
      post(6),
    ]
    const s = open()
    await s.loadFirst()
    s.setVisible('thr-5', true)
    await s.refresh()
    flush()
    expect(getSpaceTimeline).toHaveBeenLastCalledWith(SPACE, {
      limit: TIMELINE_PAGE_LIMIT,
      after: cursorFor(6),
      refresh: ['thr-2', 'thr-3', 'thr-4', 'thr-5'],
    })
  })

  it('refresh names at most 30 posts to re-read', async () => {
    room = Array.from({ length: 50 }, (_, i) => post(i + 1, { latest_run_status: i < 35 ? 'running' : 'completed' }))
    const s = open()
    await s.loadFirst()
    s.setVisible('thr-40', true)
    await s.refresh()
    flush()
    expect(getSpaceTimeline).toHaveBeenLastCalledWith(SPACE, {
      limit: TIMELINE_PAGE_LIMIT,
      after: cursorFor(50),
      refresh: Array.from({ length: 30 }, (_, i) => `thr-${i + 1}`),
    })
  })

  it('a post that gets a new message moves to the newest page, so trimming the oldest page keeps it', async () => {
    room = Array.from({ length: 410 }, (_, i) => post(i + 1))
    const s = open()
    await s.loadFirst()
    for (let i = 0; i < 7; i += 1) await s.loadEarlier()
    expect(s.threads()).toHaveLength(400)

    // Post 15 gets a new message, so its tail is now the newest in the room.
    room = room.map((thread) =>
      thread.thread_id === 'thr-15' ? post(15, { last_message_seq: 411, updated_at: stamp(411) }) : thread,
    )
    await s.refresh()
    // A new post arrives: the window goes past the cap, and the oldest page is trimmed.
    room.push(post(412))
    await s.refresh()
    flush()

    const ids = idsOf(s.threads())
    expect(ids).toContain('thr-15')
    expect(ids).toContain('thr-412')
    expect(ids).not.toContain('thr-11')
    expect(ids).toHaveLength(352)
  })

  it('a listing that is not windowed is the whole room, with nothing to page back to', async () => {
    getSpaceTimeline.mockResolvedValueOnce(timelineOf([post(1), post(2), post(3)], { windowed: false }))
    const s = open()
    await s.loadFirst()
    await s.loadEarlier()
    flush()
    expect(s.windowed()).toBe(false)
    expect(idsOf(s.threads())).toEqual(['thr-1', 'thr-2', 'thr-3'])
    expect(s.hasEarlier()).toBe(false)
    expect(s.totalPosts()).toBe(3)
    expect(getSpaceTimeline).toHaveBeenCalledTimes(1)
  })

  it('a refresh answered as a whole listing replaces every post loaded so far', async () => {
    getSpaceTimeline
      .mockResolvedValueOnce(timelineOf([post(3), post(4)], { nextBefore: cursorFor(3), newest: cursorFor(4) }))
      .mockResolvedValueOnce(timelineOf([post(10), post(11), post(12)], { windowed: false }))
    const s = open()
    await s.loadFirst()
    await s.refresh()
    flush()
    expect(getSpaceTimeline).toHaveBeenLastCalledWith(SPACE, { limit: TIMELINE_PAGE_LIMIT, after: cursorFor(4) })
    expect(s.windowed()).toBe(false)
    expect(idsOf(s.threads())).toEqual(['thr-10', 'thr-11', 'thr-12'])
    expect(s.hasEarlier()).toBe(false)
  })

  it('past 400 posts the newest page is dropped whole, and the room says there is newer to read', async () => {
    const s = await readFullRoomPastTheCap()
    expect(getSpaceTimeline).toHaveBeenCalledTimes(9)
    const ids = idsOf(s.threads())
    expect(ids).toHaveLength(TIMELINE_MAX_POSTS)
    expect(ids[0]).toBe('thr-51')
    expect(ids[ids.length - 1]).toBe('thr-450')
    expect(s.hasNewer()).toBe(true)
    expect(s.hasEarlier()).toBe(true)
    // The window lives in memory only: a Zero Data Retention room leaves nothing in browser storage.
    expect(window.localStorage.length).toBe(0)
  })

  it('loadLater continues from the newest loaded cursor, and clears hasNewer once the newest page is back', async () => {
    const s = await readFullRoomPastTheCap()
    await s.loadLater()
    flush()
    expect(getSpaceTimeline).toHaveBeenLastCalledWith(SPACE, { limit: TIMELINE_PAGE_LIMIT, after: cursorFor(450) })
    const ids = idsOf(s.threads())
    expect(ids).toHaveLength(TIMELINE_MAX_POSTS)
    expect(ids[0]).toBe('thr-101')
    expect(ids[ids.length - 1]).toBe('thr-500')
    expect(s.hasNewer()).toBe(false)
  })

  it('a 404 from the timeline propagates to the caller, which reads it as "use the legacy listing"', async () => {
    const missing = new ApiError('space_timeline_disabled', 404, 'space_timeline_disabled')
    getSpaceTimeline.mockRejectedValueOnce(missing)
    const s = open()
    await expect(s.loadFirst()).rejects.toBe(missing)
    expect(s.threads()).toEqual([])

    getSpaceTimeline.mockResolvedValueOnce(timelineOf([post(2)], { nextBefore: cursorFor(2), newest: cursorFor(2) }))
    await s.loadFirst()
    getSpaceTimeline.mockRejectedValueOnce(missing)
    await expect(s.loadEarlier()).rejects.toBe(missing)
  })

  it('ensureTranscript runs four reads at a time, newest post first, and re-reads a post whose revision changed', async () => {
    room = Array.from({ length: 8 }, (_, i) => post(i + 1))
    const s = open()
    await s.loadFirst()

    const started: string[] = []
    const reading: Array<{ readonly threadId: string; readonly resolve: (load: SpaceTranscriptLoad) => void }> = []
    loadSpaceTranscript.mockImplementation((_space: string, threadId: string) => {
      started.push(threadId)
      return new Promise<SpaceTranscriptLoad>((resolve) => reading.push({ threadId, resolve }))
    })
    // Completes one read, then checks that the reads still in flight never exceed the limit.
    const finish = async (threadId: string) => {
      const index = reading.findIndex((read) => read.threadId === threadId)
      const read = reading.splice(index, 1)[0]
      if (!read) throw new Error(`no read in flight for ${threadId}`)
      read.resolve(okRead(threadId))
      await settle()
      expect(reading.length).toBeLessThanOrEqual(TRANSCRIPT_CONCURRENCY)
    }

    const threads = s.threads()
    const first = firstOf(threads)
    for (const thread of threads.slice(0, 4)) s.ensureTranscript(thread)
    for (const thread of threads.slice(4, 7)) s.ensureTranscript(thread)
    expect(started).toEqual(['thr-1', 'thr-2', 'thr-3', 'thr-4'])

    // Each finished read frees one slot, and the slot goes to the newest post still waiting.
    await finish('thr-1')
    expect(started).toEqual(['thr-1', 'thr-2', 'thr-3', 'thr-4', 'thr-7'])
    await finish('thr-2')
    expect(started.slice(-1)).toEqual(['thr-6'])
    await finish('thr-3')
    expect(started.slice(-1)).toEqual(['thr-5'])
    for (const id of ['thr-4', 'thr-7', 'thr-6', 'thr-5']) await finish(id)
    expect(reading).toHaveLength(0)
    expect(started).toHaveLength(7)

    // The same revision is not read again; a changed one is.
    const before = started.length
    s.ensureTranscript(first)
    await settle()
    expect(started).toHaveLength(before)

    s.ensureTranscript({ ...first, updated_at: stamp(50) })
    await settle()
    expect(started.slice(before)).toEqual(['thr-1'])
    // The turns already on screen stay while the re-read is in flight.
    flush()
    expect(s.transcript('thr-1')?.loading).toBe(true)
    expect(s.transcript('thr-1')?.turns.map((item) => item.content)).toEqual(['Post 1'])
  })

  it('loadEarlierTurns slots the earlier turns in after the first turn, drops a turn it already has, and keeps the first turn first', async () => {
    room = [post(9)]
    loadSpaceTranscript
      .mockResolvedValueOnce({
        state: 'ok',
        threadId: 'thr-9',
        turns: [turn('m0'), turn('m5'), turn('m6'), turn('m7'), turn('m8')],
        window: { omittedBefore: 4, hasMoreBefore: true },
      })
      .mockResolvedValueOnce({
        state: 'ok',
        threadId: 'thr-9',
        turns: [turn('m2'), turn('m3'), turn('m4'), turn('m5')],
        window: { omittedBefore: 0, hasMoreBefore: true },
      })
      .mockResolvedValueOnce({
        state: 'ok',
        threadId: 'thr-9',
        turns: [turn('m0'), turn('m1')],
        window: { omittedBefore: 0, hasMoreBefore: false },
      })
    const s = open()
    await s.loadFirst()
    s.ensureTranscript(firstOf(s.threads()))
    await settle()
    expect(s.transcript('thr-9')?.omittedBefore).toBe(4)

    await s.loadEarlierTurns('thr-9')
    flush()
    expect(loadSpaceTranscript).toHaveBeenNthCalledWith(2, SPACE, 'thr-9', { tail: TAIL_TURNS, before: 'm5' })
    expect(messageIdsOf(s, 'thr-9')).toEqual(['m0', 'm2', 'm3', 'm4', 'm5', 'm6', 'm7', 'm8'])
    expect(s.transcript('thr-9')?.omittedBefore).toBe(1)

    await s.loadEarlierTurns('thr-9')
    flush()
    expect(loadSpaceTranscript).toHaveBeenNthCalledWith(3, SPACE, 'thr-9', { tail: TAIL_TURNS, before: 'm2' })
    expect(messageIdsOf(s, 'thr-9')).toEqual(['m0', 'm1', 'm2', 'm3', 'm4', 'm5', 'm6', 'm7', 'm8'])
    expect(s.transcript('thr-9')?.omittedBefore).toBe(0)
  })
})

// WS15-S11: the room's system rows (membership changes and notices) sit in the timeline with the posts.
describe('room system rows in the timeline store (WS15-S11)', () => {
  it('rows are de-duplicated by id across pages, and held back below the oldest loaded post while older posts remain', async () => {
    getSpaceTimeline
      .mockResolvedValueOnce(
        timelineOf(
          [post(5), post(6)],
          { nextBefore: cursorFor(5), newest: cursorFor(6) },
          [systemRow('row-old', 4), systemRow('row-new', 6)],
        ),
      )
      .mockResolvedValueOnce(
        timelineOf(
          [post(3), post(4)],
          { nextBefore: null, newest: cursorFor(4) },
          [systemRow('row-older', 2), systemRow('row-new', 6), systemRow('row-old', 4)],
        ),
      )
    const s = open()
    await s.loadFirst()
    flush()
    // Older posts are still on the server, so the row older than the oldest loaded post waits for them.
    expect(idsOf(s.systemEntries())).toEqual(['row-new'])

    await s.loadEarlier()
    flush()
    // Nothing older is left, so every row shows, each one once.
    expect(idsOf(s.systemEntries())).toEqual(['row-older', 'row-old', 'row-new'])
  })

  it('the room keeps at most 500 system rows and drops the oldest first', async () => {
    const rows = Array.from({ length: 520 }, (_, i) => systemRow(`row-${String(i + 1).padStart(3, '0')}`, i + 1))
    getSpaceTimeline.mockResolvedValueOnce(timelineOf([post(1)], { nextBefore: null, newest: cursorFor(1) }, rows))
    const s = open()
    await s.loadFirst()
    flush()
    const kept = idsOf(s.systemEntries())
    expect(kept).toHaveLength(500)
    expect(kept[0]).toBe('row-021')
    expect(kept[kept.length - 1]).toBe('row-520')
  })
})
