import { describe, expect, it } from 'vitest'

import { unreadThreadIds } from './space-unread'

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
})
