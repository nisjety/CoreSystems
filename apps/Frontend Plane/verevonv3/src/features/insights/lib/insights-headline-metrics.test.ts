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
  return { id, label: '', definition: 'Recorded source events.', metric, surface: 'inbox', unit: 'count', value, source: 'conversation-core', eventCount: 1, aggregation: 'sum_of_recorded_events' }
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
      meteredEntryCount: 0,
      subscriptionEntryCount: 0,
      unknownBasisEntryCount: 0,
      meteredCostUsd: 0,
      subscriptionInputTokens: 0,
      subscriptionOutputTokens: 0,
      unknownBasisInputTokens: 0,
      unknownBasisOutputTokens: 0,
      zeroTokenEntryCount: 0,
      latestEntryAt: null,
      basisBreakdownAvailable: true,
      ...data,
    },
    message: '',
    state,
  }
}

describe('buildHeadlineMetrics', () => {
  it('uses verified owner outcomes independently of unrelated partial event series', () => {
    const verified = (id: string, value: number) => ({ ...scorecard(`inbox.${id}`, id, value), coverage: 'measured' as const, aggregation: 'owner_record_count' })
    const metrics = buildHeadlineMetrics(overview([
      verified('tickets_currently_resolved', 0), verified('ai_reviews_approved', 3), verified('ai_reviews_rejected', 1),
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 100),
    ], 'partial'), costSummary({ entryCount: 1, unknownBasisEntryCount: 1 }, 'partial'))
    expect(metrics[0]).toMatchObject({ state: 'live', value: '0', label: 'Resolved tickets' })
    expect(metrics[1]).toMatchObject({ state: 'live', value: '75.0%' })
    expect(metrics[2]?.state).toBe('partial')
  })

  it('requires both verified review counts before presenting a confirmed approval rate', () => {
    const metrics = buildHeadlineMetrics(overview([
      { ...scorecard('inbox.ai_reviews_approved', 'ai_reviews_approved', 3), coverage: 'measured' },
    ], 'partial'), costSummary())
    expect(metrics[1]?.state).toBe('partial')
    expect(metrics[1]?.value).toBe('—')
  })
  it('reports all three metrics unavailable when the overview resource failed', () => {
    const metrics = buildHeadlineMetrics(overview([], 'unavailable'), costSummary({}, 'unavailable'))

    expect(metrics.map((m) => m.id)).toEqual(['conversations_handled', 'ai_draft_acceptance', 'ai_usage_cost'])
    expect(metrics[0]?.state).toBe('unavailable')
    expect(metrics[1]?.state).toBe('unavailable')
    expect(metrics[2]?.state).toBe('unavailable')
    expect(metrics.every((m) => m.value === '—')).toBe(true)
  })

  it('reports an honest empty state for ticket-resolution events and approval when no rows exist', () => {
    const metrics = buildHeadlineMetrics(overview([], 'empty'), costSummary())

    const conversations = metrics.find((m) => m.id === 'conversations_handled')
    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(conversations?.state).toBe('empty')
    expect(conversations?.value).toBe('—')
    expect(acceptance?.state).toBe('empty')
    expect(acceptance?.value).toBe('—')
  })

  it('does not turn an unverified empty review window into a 0 of 0 approval result', () => {
    const metrics = buildHeadlineMetrics(overview([], 'partial'), costSummary())
    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')

    expect(acceptance?.state).toBe('partial')
    expect(acceptance?.value).toBe('—')
    expect(acceptance?.detail).toContain('not a confirmed zero')
    expect(acceptance?.detail).not.toContain('0 of 0')
  })

  it('does not claim observed ticket events when no ticket scorecard was returned', () => {
    const metrics = buildHeadlineMetrics(overview([], 'partial'), costSummary())
    const tickets = metrics.find((m) => m.id === 'conversations_handled')

    expect(tickets?.value).toBe('—')
    expect(tickets?.detail).toContain('No resolved-ticket events were observed')
    expect(tickets?.detail).toContain('not a confirmed zero')
  })

  it('surfaces the real tickets_resolved count with its actual meaning', () => {
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

  it('shows observed ticket and approval data while keeping unknown coverage explicit', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.tickets_resolved', 'tickets_resolved', 7),
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 3),
      scorecard('inbox.ai_actions_rejected', 'ai_actions_rejected', 1),
    ], 'partial'), costSummary())

    const tickets = metrics.find((m) => m.id === 'conversations_handled')
    const approval = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(tickets?.state).toBe('partial')
    expect(tickets?.value).toBe('Observed 7')
    expect(tickets?.detail).toContain('may not be the full period total')
    expect(approval?.state).toBe('partial')
    expect(approval?.value).toBe('Observed 75.0%')
    expect(approval?.detail).toContain('3 of 4')
    expect(approval?.detail).toContain('may be incomplete')
  })

  it('treats zero approved and zero rejected as not-yet-reporting, never a fabricated 0%', () => {
    const metrics = buildHeadlineMetrics(overview([
      scorecard('inbox.ai_actions_approved', 'ai_actions_approved', 0),
    ]), costSummary())

    const acceptance = metrics.find((m) => m.id === 'ai_draft_acceptance')
    expect(acceptance?.state).toBe('empty')
    expect(acceptance?.value).toBe('—')
  })

  it('reports only metered spend and keeps it separate from subscription usage', () => {
    const metrics = buildHeadlineMetrics(overview([]), costSummary({
      entryCount: 4,
      totalCostUsd: 0.0125,
      totalInputTokens: 1200,
      totalOutputTokens: 340,
      meteredEntryCount: 4,
      meteredCostUsd: 0.0125,
    }))

    const cost = metrics.find((m) => m.id === 'ai_usage_cost')
    expect(cost?.state).toBe('live')
    expect(cost?.value).toBe('4 requests')
    expect(cost?.summary).toContain('API estimate ≈ $0.01')
    expect(cost?.detail).toContain('4 metered requests')
  })

  it('shows an honest empty state when cost-core has no ledger rows', () => {
    const metrics = buildHeadlineMetrics(overview([]), costSummary())

    const cost = metrics.find((m) => m.id === 'ai_usage_cost')
    expect(cost?.state).toBe('empty')
    expect(cost?.value).toBe('0 requests')
    expect(cost?.value).not.toContain('$')
  })

  it('does not present a partial metered subtotal as total spend', () => {
    const metrics = buildHeadlineMetrics(overview([]), costSummary({
      entryCount: 150,
      unknownBasisEntryCount: 150,
      zeroTokenEntryCount: 33,
      totalInputTokens: 558348,
      totalOutputTokens: 23867,
      meteredCostUsd: 0,
    }))

    const cost = metrics.find((m) => m.id === 'ai_usage_cost')
    expect(cost?.state).toBe('partial')
    expect(cost?.value).toBe('150 requests')
    expect(cost?.value).not.toContain('$0')
    expect(cost?.detail).toContain('150 entries have unknown billing basis')
    expect(cost?.detail).toContain('33 entries have no token counts')
  })

  it('identifies organization-wide usage returned by the authorized cost endpoint', () => {
    const metrics = buildHeadlineMetrics(overview([]), costSummary({ scope: 'organization', entryCount: 1, subscriptionEntryCount: 1 }))
    const cost = metrics.find((m) => m.id === 'ai_usage_cost')
    expect(cost?.label).toBe('Organization AI usage')
    expect(cost?.value).toBe('1 request')
    expect(cost?.summary).not.toContain('API estimate')
  })
})
