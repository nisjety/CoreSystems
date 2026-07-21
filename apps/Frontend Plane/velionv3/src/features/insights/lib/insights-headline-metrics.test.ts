import { describe, expect, it } from 'vitest'
import { buildHeadlineMetrics } from '@/features/insights/lib/insights-headline-metrics'
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

describe('buildHeadlineMetrics', () => {
  it('reports all three metrics unavailable when the overview resource failed', () => {
    const metrics = buildHeadlineMetrics(overview([], 'unavailable'))

    expect(metrics.map((m) => m.id)).toEqual(['conversations_handled', 'ai_draft_acceptance', 'cost_per_resolution'])
    expect(metrics[0]?.state).toBe('unavailable')
    expect(metrics[1]?.state).toBe('unavailable')
    // cost_per_resolution stays its own honest `planned` state even when the
    // overview itself is unavailable — it never had a data path to begin with.
    expect(metrics[2]?.state).toBe('planned')
    expect(metrics.every((m) => m.value === '—')).toBe(true)
  })

  it('reports an honest empty state for conversations handled and acceptance when no rows exist', () => {
    const metrics = buildHeadlineMetrics(overview([], 'empty'))

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
    ]))

    const conversations = metrics.find((m) => m.id === 'conversations_handled')
    expect(conversations?.state).toBe('live')
    expect(conversations?.value).toBe('7')
  })

  it('computes AI draft acceptance percentage from real approved/rejected counts', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 3),
      scorecard('inbox.ai_actions_rejected', 'ai_actions_rejected', 1),
    ]))

    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(acceptance?.state).toBe('live')
    expect(acceptance?.value).toBe('75.0%')
    expect(acceptance?.detail).toContain('3 of 4')
  })

  it('treats zero approved and zero rejected as not-yet-reporting, never a fabricated 0%', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 0),
    ]))

    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(acceptance?.state).toBe('empty')
    expect(acceptance?.value).toBe('—')
  })

  it('always reports cost per resolution as planned/honest-empty regardless of other rows', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.tickets_resolved', 'tickets_resolved', 12),
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 5),
    ]))

    const cost = metrics.find((m) => m.id === 'cost_per_resolution')
    expect(cost?.state).toBe('planned')
    expect(cost?.value).toBe('—')
    expect(cost?.detail).toContain('cost-core')
  })
})
