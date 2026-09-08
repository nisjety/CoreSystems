import { flush } from 'solid-js'
import { beforeEach, describe, expect, it } from 'vitest'

import {
  beginOwnStream,
  endOwnStream,
  isSpaceObserved,
  isSpaceWorking,
  liveThreadsIn,
  ownStream,
  publishSpaceThreads,
  resetLiveWorkForTests,
  retractSpaceThreads,
} from './space-live-work'

const thread = (id: string, status?: string) => ({
  thread_id: id,
  space_id: 'room-1',
  latest_run_status: status,
})

// Solid 2 defers signal writes until a flush. Outside a reactive scope a
// synchronous read right after a write sees the OLD value — which is exactly
// the trap the SpacePage tests hit and solved with `flush()`. Every write here
// flushes so the assertions read what the store now holds.
const publish = (...args: Parameters<typeof publishSpaceThreads>) => { publishSpaceThreads(...args); flush() }
const retract = (...args: Parameters<typeof retractSpaceThreads>) => { retractSpaceThreads(...args); flush() }
const begin = (...args: Parameters<typeof beginOwnStream>) => { beginOwnStream(...args); flush() }
const end = () => { endOwnStream(); flush() }

beforeEach(() => { resetLiveWorkForTests(); flush() })

describe('space-live-work', () => {
  // The whole reason the store exists: an unobserved room is UNKNOWN, and a
  // consumer that reads "no live threads" as "idle" would draw a calm dot on a
  // room it has never looked at.
  it('distinguishes an unobserved room from an idle one', () => {
    expect(isSpaceObserved('room-1')).toBe(false)
    expect(liveThreadsIn('room-1')).toEqual([])
    expect(isSpaceWorking('room-1')).toBe(false)

    publish('room-1', [thread('t1', 'completed')])
    expect(isSpaceObserved('room-1')).toBe(true)
    expect(isSpaceWorking('room-1')).toBe(false)
  })

  it('reports only threads whose run is actually producing output', () => {
    publish('room-1', [
      thread('running', 'running'),
      thread('queued', 'queued'),
      // Paused on a person: active, but nobody is working.
      thread('paused', 'awaiting_approval'),
      thread('done', 'completed'),
      thread('stopped', 'cancelled'),
    ])
    expect(liveThreadsIn('room-1').map((t) => t.thread_id)).toEqual(['running', 'queued'])
    expect(isSpaceWorking('room-1')).toBe(true)
  })

  // The sender's own stream is immediate; the server's projection arrives on
  // the poll. They are different kinds of knowledge and stay separate.
  it('keeps the browser’s own stream apart from the server’s projection', () => {
    publish('room-1', [thread('t1', 'completed')])
    expect(isSpaceWorking('room-1')).toBe(false)

    begin({ spaceRef: 'room-1', threadId: 't1' })
    expect(ownStream()).toEqual({ spaceRef: 'room-1', threadId: 't1' })
    // The projection still says nothing is live — that is the server's truth.
    expect(liveThreadsIn('room-1')).toEqual([])
    // But the room IS working, from the browser's own knowledge.
    expect(isSpaceWorking('room-1')).toBe(true)
    // And only this room.
    expect(isSpaceWorking('room-2')).toBe(false)

    end()
    expect(ownStream()).toBeUndefined()
    expect(isSpaceWorking('room-1')).toBe(false)
  })

  // A page that left the room stops refreshing its projection. Leaving the
  // stale copy in place would keep the sidebar saying "busy" indefinitely.
  it('forgets a room once its publisher retracts it', () => {
    publish('room-1', [thread('t1', 'running')])
    expect(isSpaceWorking('room-1')).toBe(true)

    retract('room-1')
    expect(isSpaceObserved('room-1')).toBe(false)
    expect(isSpaceWorking('room-1')).toBe(false)
  })

  it('replaces, not merges, a room’s projection on each publish', () => {
    publish('room-1', [thread('t1', 'running')])
    publish('room-1', [thread('t1', 'completed')])
    expect(liveThreadsIn('room-1')).toEqual([])
  })

  it('ignores a blank space reference rather than storing it', () => {
    publish('   ', [thread('t1', 'running')])
    expect(isSpaceObserved('')).toBe(false)
    expect(isSpaceObserved('   ')).toBe(false)
  })
})
