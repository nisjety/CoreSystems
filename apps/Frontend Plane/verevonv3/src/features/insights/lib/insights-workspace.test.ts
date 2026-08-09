import { describe, expect, it } from 'vitest'
import {
  costResult,
  overviewResult,
  withResourceTimeout,
  type ResourceResult,
} from '@/features/insights/lib/insights-workspace'
import type { InsightScorecard } from '@/shared/api/insights-client'

describe('overviewResult', () => {
  it('maps recorded scorecards to a live result that carries the real rows', () => {
    const scorecards: InsightScorecard[] = [
      { id: 'inbox.a', label: 'Inbox a', metric: 'a', surface: 'inbox', unit: 'count', value: 4, source: 'conversation-core' },
    ]

    const result = overviewResult(scorecards)

    expect(result.state).toBe('live')
    expect(result.data).toEqual(scorecards)
  })

  it('maps zero scorecards to an honest empty state, never live', () => {
    const result = overviewResult([])

    expect(result.state).toBe('empty')
    expect(result.data).toEqual([])
  })
})

describe('costResult', () => {
  it('marks a cost-core rollup with real ledger entries as live', () => {
    const result = costResult({ entryCount: 2, totalCostUsd: 0.004, totalInputTokens: 100, totalOutputTokens: 20 })

    expect(result.state).toBe('live')
    expect(result.data.totalCostUsd).toBe(0.004)
  })

  it('marks a zeroed cost-core rollup as empty instead of showing a fabricated $0 metric', () => {
    const result = costResult({ entryCount: 0, totalCostUsd: 0, totalInputTokens: 0, totalOutputTokens: 0 })

    expect(result.state).toBe('empty')
  })
})

describe('withResourceTimeout', () => {
  it('falls back when a resource read does not settle', async () => {
    const fallback: ResourceResult<string[]> = { data: [], message: 'Timed out.', state: 'unavailable' }

    await expect(withResourceTimeout(new Promise<ResourceResult<string[]>>(() => undefined), fallback, 1))
      .resolves
      .toBe(fallback)
  })
})
