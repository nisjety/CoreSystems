import { flush } from 'solid-js'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { createSpaceSendJournal, type SpaceSendJournal } from './space-send-journal'

// Solid 2 batches signal writes until the next flush; flush after each write so a read sees it.
function makeJournal(): SpaceSendJournal {
  const inner = createSpaceSendJournal()
  const settled = <A extends unknown[], R>(fn: (...args: A) => R) => (...args: A): R => {
    const result = fn(...args)
    flush()
    return result
  }
  return {
    ...inner,
    begin: settled(inner.begin),
    update: settled(inner.update),
    remove: settled(inner.remove),
    reconcile: settled(inner.reconcile),
  }
}

afterEach(() => {
  vi.useRealTimers()
})

const failure = { code: 'network', message: 'No answer', retryable: true } as const

describe('space send journal (WS01-S6)', () => {
  describe('begin, update, remove', () => {
    it('records a send as sending by default and reads it back by id and by space', () => {
      const journal = makeJournal()
      const entry = journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'hi' })
      expect(entry.stage).toBe('sending')
      expect(journal.get('s1')).toEqual(entry)
      expect(journal.entries()).toHaveLength(1)
      expect(journal.forSpace('room')).toHaveLength(1)
      expect(journal.forSpace('other')).toEqual([])
    })

    it('honours an explicit stage and keeps thread and baseline only when given', () => {
      const journal = makeJournal()
      const plain = journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'hi', stage: 'queued' })
      expect(plain.stage).toBe('queued')
      expect('threadId' in plain).toBe(false)
      expect('baselineUpdatedAt' in plain).toBe(false)
      const reply = journal.begin({
        sendId: 's2',
        kind: 'post_reply',
        spaceRef: 'room',
        threadId: 'thr_1',
        content: 'again',
        baselineUpdatedAt: '2026-10-27T10:00:00Z',
      })
      expect(reply.threadId).toBe('thr_1')
      expect(reply.baselineUpdatedAt).toBe('2026-10-27T10:00:00Z')
    })

    it('replaces an entry that has the same send id', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'first' })
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'second' })
      expect(journal.entries()).toHaveLength(1)
      expect(journal.get('s1')?.content).toBe('second')
    })

    it('updates one entry and leaves the others alone', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      journal.begin({ sendId: 's2', kind: 'post_new', spaceRef: 'room', content: 'b' })
      journal.update('s1', { stage: 'accepted', threadId: 'thr_1' })
      expect(journal.get('s1')).toMatchObject({ stage: 'accepted', threadId: 'thr_1' })
      expect(journal.get('s2')?.stage).toBe('sending')
    })

    it('ignores an update for an unknown id', () => {
      const journal = makeJournal()
      journal.update('missing', { stage: 'failed', failure })
      expect(journal.entries()).toEqual([])
    })

    it('removes an entry and tolerates removing one that is not there', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      const before = journal.entries()
      journal.remove('nope')
      expect(journal.entries()).toBe(before)
      journal.remove('s1')
      expect(journal.entries()).toEqual([])
    })

    it('stamps createdAt from the clock', () => {
      vi.useFakeTimers()
      vi.setSystemTime(new Date('2026-10-27T12:00:00Z'))
      const journal = makeJournal()
      const entry = journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      expect(entry.createdAt).toBe(Date.parse('2026-10-27T12:00:00Z'))
    })
  })

  describe('failure belongs to the failed stage only', () => {
    it('keeps a failure while the stage is failed', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      journal.update('s1', { stage: 'failed', failure })
      expect(journal.get('s1')?.failure).toEqual(failure)
    })

    it('clears it when the stage leaves failed', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      journal.update('s1', { stage: 'failed', failure })
      journal.update('s1', { stage: 'sending' })
      expect(journal.get('s1')?.failure).toBeUndefined()
      journal.update('s1', { stage: 'failed', failure })
      journal.update('s1', { stage: 'accepted' })
      expect(journal.get('s1')?.failure).toBeUndefined()
    })
  })

  describe('reconcile', () => {
    it('drops an accepted post_new when its thread is listed', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      journal.update('s1', { stage: 'accepted', threadId: 'thr_1' })
      journal.reconcile('room', [{ thread_id: 'thr_1' }])
      expect(journal.get('s1')).toBeUndefined()
    })

    it('keeps an accepted post_new while its thread is not listed yet', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      journal.update('s1', { stage: 'accepted', threadId: 'thr_1' })
      journal.reconcile('room', [{ thread_id: 'thr_other' }])
      expect(journal.get('s1')).toBeDefined()
    })

    it('drops an accepted post_reply only once the thread updated_at moved past the baseline', () => {
      const journal = makeJournal()
      journal.begin({
        sendId: 's1',
        kind: 'post_reply',
        spaceRef: 'room',
        threadId: 'thr_1',
        content: 'a',
        baselineUpdatedAt: '2026-10-27T10:00:00Z',
      })
      journal.update('s1', { stage: 'accepted' })
      journal.reconcile('room', [{ thread_id: 'thr_1', updated_at: '2026-10-27T10:00:00Z' }])
      expect(journal.get('s1')).toBeDefined()
      journal.reconcile('room', [{ thread_id: 'thr_1', updated_at: '2026-10-27T10:05:00Z' }])
      expect(journal.get('s1')).toBeUndefined()
    })

    it('treats a reply with no baseline and no updated_at as not yet shown', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_reply', spaceRef: 'room', threadId: 'thr_1', content: 'a' })
      journal.update('s1', { stage: 'accepted' })
      journal.reconcile('room', [{ thread_id: 'thr_1' }])
      expect(journal.get('s1')).toBeDefined()
    })

    it('never drops a queued, sending or failed entry', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 'q', kind: 'post_new', spaceRef: 'room', threadId: 'thr_1', content: 'a', stage: 'queued' })
      journal.begin({ sendId: 's', kind: 'post_new', spaceRef: 'room', threadId: 'thr_1', content: 'b' })
      journal.begin({ sendId: 'f', kind: 'post_new', spaceRef: 'room', threadId: 'thr_1', content: 'c' })
      journal.update('f', { stage: 'failed', failure })
      journal.reconcile('room', [{ thread_id: 'thr_1', updated_at: '2030-01-01T00:00:00Z' }])
      expect(journal.entries().map((entry) => entry.sendId).sort()).toEqual(['f', 'q', 's'])
    })

    it('never touches another space', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'other', content: 'a' })
      journal.update('s1', { stage: 'accepted', threadId: 'thr_1' })
      journal.reconcile('room', [{ thread_id: 'thr_1' }])
      expect(journal.get('s1')).toBeDefined()
    })

    it('keeps the same entries array when nothing is dropped', () => {
      const journal = makeJournal()
      journal.begin({ sendId: 's1', kind: 'post_new', spaceRef: 'room', content: 'a' })
      const before = journal.entries()
      journal.reconcile('room', [{ thread_id: 'thr_1' }])
      expect(journal.entries()).toBe(before)
    })
  })
})
