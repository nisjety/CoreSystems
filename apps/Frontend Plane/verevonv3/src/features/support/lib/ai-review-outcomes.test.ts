import { describe, expect, it } from 'vitest'
import { deriveReviewDecisionTiming, formatReviewTiming } from './ai-review-outcomes'

describe('deriveReviewDecisionTiming', () => {
  it('returns the median duration only for actions with a persisted terminal decision or outcome', () => {
    const timing = deriveReviewDecisionTiming([
      { status: 'approved', created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:01:00.000Z' },
      { status: 'rejected', created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:05:00.000Z' },
      { status: 'executed', created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:09:00.000Z' },
      { status: 'suggested', created_at: '2026-08-03T10:00:00.000Z', updated_at: '2026-08-03T10:30:00.000Z' },
    ])

    expect(timing).toEqual({ measuredActions: 3, medianMilliseconds: 300_000 })
    expect(formatReviewTiming(timing.medianMilliseconds)).toBe('5 min')
  })

  it('excludes invalid and negative timestamp pairs instead of treating them as instantaneous decisions', () => {
    const timing = deriveReviewDecisionTiming([
      { status: 'failed', created_at: 'not-a-date', updated_at: '2026-08-03T10:02:00.000Z' },
      { status: 'approved', created_at: '2026-08-03T10:03:00.000Z', updated_at: '2026-08-03T10:02:00.000Z' },
    ])

    expect(timing).toEqual({ measuredActions: 0, medianMilliseconds: null })
    expect(formatReviewTiming(timing.medianMilliseconds)).toBe('—')
  })
})
