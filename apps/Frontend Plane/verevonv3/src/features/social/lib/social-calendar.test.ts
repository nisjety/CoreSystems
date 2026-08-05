import { describe, expect, it } from 'vitest'
import {
  buildCalendarDays,
  currentCalendarMonth,
  defaultDraftScheduledAt,
  postsByStatus,
  postsForDate,
} from '@/features/social/lib/social-calendar'
import type { SocialPost } from '@/shared/api/social-client'

const basePost: SocialPost = {
  id: 'post_1',
  title: 'Post',
  body: 'Body',
  status: 'scheduled',
  scheduledAt: '2026-06-16T08:30:00.000Z',
  platforms: ['linkedin'],
  source: { kind: 'manual', label: 'Manual' },
  approval: { required: true, state: 'approved' },
  media: [],
}

describe('social calendar helpers', () => {
  it('uses the current clock for the initial month and a new draft timestamp', () => {
    const now = new Date('2026-08-03T14:27:00.000Z')

    expect(currentCalendarMonth(now).toISOString()).toBe('2026-08-01T00:00:00.000Z')
    expect(defaultDraftScheduledAt(now)).toBe('2026-08-03T14:27:00.000Z')
  })

  it('builds a stable six-week month grid starting on Monday', () => {
    const days = buildCalendarDays(new Date('2026-06-15T00:00:00.000Z'), new Date('2026-06-15T08:00:00.000Z'))

    expect(days).toHaveLength(42)
    expect(days[0]?.isoDate).toBe('2026-06-01')
    expect(days[0]?.inMonth).toBe(true)
    expect(days[14]?.isToday).toBe(true)
  })

  it('groups posts by scheduled date and status', () => {
    const posts: SocialPost[] = [
      basePost,
      { ...basePost, id: 'post_2', status: 'draft', scheduledAt: '2026-06-17T10:00:00.000Z' },
    ]

    expect(postsForDate(posts, '2026-06-16').map((post) => post.id)).toEqual(['post_1'])
    expect(postsByStatus(posts)).toMatchObject({ scheduled: 1, draft: 1, failed: 0 })
  })
})
