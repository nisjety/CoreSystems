import type { SocialPost } from '@/shared/api/social-client'

export type CalendarDay = {
  isoDate: string
  dayNumber: number
  inMonth: boolean
  isToday: boolean
}

const dayMs = 24 * 60 * 60 * 1000

/**
 * The calendar operates on UTC day keys, so derive its initial viewport from
 * the live clock in the same timezone. Keeping this in one helper prevents a
 * deterministic test fixture from becoming a production default again.
 */
export function currentCalendarMonth(now = new Date()) {
  return new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), 1))
}

/**
 * Drafts need a real temporal anchor for their calendar placement. A draft is
 * not a provider schedule, but its timestamp must never point at a stale
 * fixture month or imply a past planned send.
 */
export function defaultDraftScheduledAt(now = new Date()) {
  return now.toISOString()
}

export function monthLabel(month: Date) {
  return month.toLocaleDateString('en-GB', { month: 'long', year: 'numeric' })
}

export function buildCalendarDays(month: Date, today = new Date()): CalendarDay[] {
  const first = new Date(Date.UTC(month.getUTCFullYear(), month.getUTCMonth(), 1))
  const startOffset = (first.getUTCDay() + 6) % 7
  const start = new Date(first.getTime() - startOffset * dayMs)
  const todayIso = toIsoDate(today)

  return Array.from({ length: 42 }, (_, index) => {
    const date = new Date(start.getTime() + index * dayMs)
    return {
      isoDate: toIsoDate(date),
      dayNumber: date.getUTCDate(),
      inMonth: date.getUTCMonth() === month.getUTCMonth(),
      isToday: toIsoDate(date) === todayIso,
    }
  })
}

export function postsForDate(posts: readonly SocialPost[], isoDate: string) {
  return posts.filter((post) => toIsoDate(new Date(post.scheduledAt)) === isoDate)
}

export function postsByStatus(posts: readonly SocialPost[]) {
  return posts.reduce<Record<SocialPost['status'], number>>((accumulator, post) => {
    accumulator[post.status] = (accumulator[post.status] ?? 0) + 1
    return accumulator
  }, {
    draft: 0,
    pending_approval: 0,
    scheduled: 0,
    publishing: 0,
    published: 0,
    failed: 0,
    blocked: 0,
  })
}

export function toIsoDate(date: Date) {
  return date.toISOString().slice(0, 10)
}
