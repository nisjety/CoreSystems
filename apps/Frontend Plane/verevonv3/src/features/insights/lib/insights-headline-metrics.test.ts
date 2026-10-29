import { describe, expect, it } from 'vitest'
import { buildHeadlineMetrics } from '@/features/insights/lib/insights-headline-metrics'
import type { CostSummary } from '@/shared/api/cost-client'
import type { InsightScorecard } from '@/shared/api/insights-client'
import type { ResourceResult } from '@/shared/read-data'

function overview(
  data: InsightScorecard[],
  state: ResourceResult<InsightScorecard[]>['state'] = 'live',
): ResourceResult<InsightScorecard[]> {
  return { data, message: '', state }
}

function scorecard(id: string, metric: string, value: number): InsightScorecard {
  return { id, label: '', metric, surface: 'inbox', unit: 'count', value, source: 'conversation-core' }
}

function costSummary(
  data: Partial<CostSummary> = {},
  state: ResourceResult<CostSummary>['state'] = 'live',
): ResourceResult<CostSummary> {
  return {
    data: {
      totalInputTokens: 0,
      totalOutputTokens: 0,
      totalCostUsd: 0,
      entryCount: 0,
      ...data,
    },
    message: '',
    state,
  }
}

describe('buildHeadlineMetrics', () => {
  it('reports all three metrics unavailable when the overview resource failed', () => {
    const metrics = buildHeadlineMetrics(overview([], 'unavailable'), costSummary({}, 'unavailable'))

    expect(metrics.map((m) => m.id)).toEqual(['conversations_handled', 'ai_draft_acceptance', 'ai_usage_cost'])
    expect(metrics[0]?.state).toBe('unavailable')
    expect(metrics[1]?.state).toBe('unavailable')
    expect(metrics[2]?.state).toBe('unavailable')
    expect(metrics.every((m) => m.value === '—')).toBe(true)
  })

  it('reports an honest empty state for conversations handled and acceptance when no rows exist', () => {
    const metrics = buildHeadlineMetrics(overview([], 'empty'), costSummary())

    const conversations = metrics.find((m) => m.id === 'conversations_handled')
    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(conversations?.state).toBe('empty')
    expect(conversations?.value).toBe('—')
    expect(acceptance?.state).toBe('empty')
    expect(acceptance?.value).toBe('—')
  })

  it('surfaces the real tickets_resolved count as conversations handled', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.tickets_resolved', 'tickets_resolved', 7),
    ]), costSummary())

    const conversations = metrics.find((m) => m.id === 'conversations_handled')
    expect(conversations?.state).toBe('live')
    expect(conversations?.value).toBe('7')
  })

  it('computes AI draft acceptance percentage from real approved/rejected counts', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 3),
      scorecard('inbox.ai_actions_rejected', 'ai_actions_rejected', 1),
    ]), costSummary())

    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(acceptance?.state).toBe('live')
    expect(acceptance?.value).toBe('75.0%')
    expect(acceptance?.detail).toContain('3 of 4')
  })

  it('treats zero approved and zero rejected as not-yet-reporting, never a fabricated 0%', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 0),
    ]), costSummary())

    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(acceptance?.state).toBe('empty')
    expect(acceptance?.value).toBe('—')
  })

  it('reports the real cost-core ledger total without mislabelling it as a resolution cost', () => {
    const metrics = buildHeadlineMetrics(overview([]), costSummary({
      entryCount: 4,
      totalCostUsd: 0.0125,
      totalInputTokens: 1200,
      totalOutputTokens: 340,
    }))

    const cost = metrics.find((m) => m.id === 'ai_usage_cost')
    expect(cost?.state).toBe('live')
    expect(cost?.value).toBe('$0.0125')
    expect(cost?.detail).toContain('4 cost-bearing AI requests')
  })

  it('shows an honest empty state when cost-core has no ledger rows', () => {
    const metrics = buildHeadlineMetrics(overview([]), costSummary())

    const cost = metrics.find((m) => m.id === 'ai_usage_cost')
    expect(cost?.state).toBe('empty')
    expect(cost?.value).toBe('—')
  })
})
