import { describe, expect, it } from 'vitest'

import { firstUnreadIndex, markUnreadBeforeMs, unreadThreadIds } from './space-unread'

const thread = (id: string, updated_at?: string, extra: Record<string, unknown> = {}) => ({
  thread_id: id,
  space_id: 'room-1',
  owner_subject_id: 'someone-else',
  updated_at,
  ...extra,
})

const marker = (iso: string) => ({ last_read_at: Date.parse(iso) })

describe('unreadThreadIds', () => {
  it('badges a post that changed after the reader last caught up', () => {
    const ids = unreadThreadIds(
      [thread('old', '2026-09-07T09:00:00Z'), thread('new', '2026-09-07T11:00:00Z')],
      marker('2026-09-07T10:00:00Z'),
      'viewer',
    )
    expect([...ids]).toEqual(['new'])
  })

  // No last visit to be new since. Badging every post on a first visit teaches
  // people to ignore the badge.
  it('badges nothing on a first visit', () => {
    expect(unreadThreadIds([thread('t1', '2026-09-07T11:00:00Z')], undefined, 'viewer').size).toBe(0)
    expect(unreadThreadIds([thread('t1', '2026-09-07T11:00:00Z')], { last_read_at: null }, 'viewer').size).toBe(0)
  })

  // Your own post arriving is not news.
  it('never badges the reader’s own posts', () => {
    const ids = unreadThreadIds(
      [
        thread('mine', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer' }),
        thread('theirs', '2026-09-07T11:00:00Z'),
      ],
      marker('2026-09-07T10:00:00Z'),
      'viewer',
    )
    expect([...ids]).toEqual(['theirs'])
  })

  // A reply landing on an old thread is what makes it new again.
  it('counts the run’s own timestamp as activity', () => {
    const ids = unreadThreadIds(
      [thread('t1', '2026-09-07T09:00:00Z', { latest_run_updated_at: '2026-09-07T10:30:00Z' })],
      marker('2026-09-07T10:00:00Z'),
      'viewer',
    )
    expect([...ids]).toEqual(['t1'])
  })

  it('leaves a post with no readable timestamp alone', () => {
    const ids = unreadThreadIds(
      [thread('t1', undefined), thread('t2', 'not a date')],
      marker('2026-09-07T10:00:00Z'),
      'viewer',
    )
    expect(ids.size).toBe(0)
  })

  // The badges point at what was new when the reader ARRIVED. The page keeps
  // advancing the durable marker while the room stays open; if the badges
  // followed it they would vanish on the first poll.
  it('is computed against the marker as it was, not as it becomes', () => {
    const threads = [thread('t1', '2026-09-07T11:00:00Z')]
    const arrival = marker('2026-09-07T10:00:00Z')
    const advanced = marker('2026-09-07T12:00:00Z')
    expect(unreadThreadIds(threads, arrival, 'viewer').has('t1')).toBe(true)
    expect(unreadThreadIds(threads, advanced, 'viewer').has('t1')).toBe(false)
  })

  // WS01-S7b: with last_author_subject_id the rule is "the latest message is
  // mine", not "I started the thread". Kept identical to the Convex test.
  describe('last_author_subject_id', () => {
    const since = marker('2026-09-07T10:00:00Z')

    it('badges a thread I started whose latest message is someone else’s', () => {
      const ids = unreadThreadIds(
        [thread('t1', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer', last_author_subject_id: 'someone-else' })],
        since,
        'viewer',
      )
      expect([...ids]).toEqual(['t1'])
    })

    it('does not badge a thread whose latest message is mine', () => {
      const ids = unreadThreadIds(
        [thread('t1', '2026-09-07T11:00:00Z', { owner_subject_id: 'someone-else', last_author_subject_id: 'viewer' })],
        since,
        'viewer',
      )
      expect(ids.size).toBe(0)
    })

    it('still applies the owner rule when the field is absent', () => {
      const ids = unreadThreadIds(
        [
          thread('mine', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer' }),
          thread('theirs', '2026-09-07T11:00:00Z'),
        ],
        since,
        'viewer',
      )
      expect([...ids]).toEqual(['theirs'])
    })

    it('treats an empty string as absent', () => {
      const ids = unreadThreadIds(
        [
          thread('mine', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer', last_author_subject_id: '' }),
          thread('blank', '2026-09-07T11:00:00Z', { owner_subject_id: 'viewer', last_author_subject_id: '  ' }),
          thread('theirs', '2026-09-07T11:00:00Z', { last_author_subject_id: '' }),
        ],
        since,
        'viewer',
      )
      expect([...ids]).toEqual(['theirs'])
    })
  })
})

// WS11-S4: where the "N new" divider goes, and the read position a "Mark unread" sets.
describe('firstUnreadIndex', () => {
  it('points at the first new post in the order the room draws them, oldest first', () => {
    const ordered = [thread('old'), thread('n1'), thread('n2')]
    expect(firstUnreadIndex(ordered, new Set(['n2', 'n1']))).toBe(1)
  })

  it('is -1 when nothing is new, when the list is empty, or when the new post is not in the list', () => {
    const ordered = [thread('old'), thread('n1')]
    expect(firstUnreadIndex(ordered, new Set<string>())).toBe(-1)
    expect(firstUnreadIndex([], new Set(['n1']))).toBe(-1)
    expect(firstUnreadIndex(ordered, new Set(['gone']))).toBe(-1)
  })
})

describe('markUnreadBeforeMs', () => {
  it('lands one millisecond before the post’s latest activity, so the post reads as new again', () => {
    expect(markUnreadBeforeMs(thread('t1', '2026-09-07T11:00:00Z'))).toBe(Date.parse('2026-09-07T11:00:00Z') - 1)
  })

  it('uses the later of the post and its latest run', () => {
    const post = thread('t1', '2026-09-07T11:00:00Z', { latest_run_updated_at: '2026-09-07T12:00:00Z' })
    expect(markUnreadBeforeMs(post)).toBe(Date.parse('2026-09-07T12:00:00Z') - 1)
  })

  it('is undefined for a post with no readable time, rather than a guess', () => {
    expect(markUnreadBeforeMs(thread('t1'))).toBeUndefined()
    expect(markUnreadBeforeMs(thread('t2', 'yesterday-ish'))).toBeUndefined()
  })

  it('never goes below zero', () => {
    expect(markUnreadBeforeMs(thread('t3', '1970-01-01T00:00:00Z'))).toBe(0)
  })
})
