import type { InsightScorecard } from '@/shared/api/insights-client'
import type { CostSummary } from '@/shared/api/cost-client'
import type { MeasurementState, ResourceResult } from '@/shared/read-data'

// The exact `surface.metric` scorecard ids insight-core's buildScorecards
// produces (apps/Application Plane/insight-core/internal/insights/service.go)
// for the three pilot headline numbers. Reading these ids from the SAME
// scorecards array the generic Insights list renders — never a separate or
// fabricated source — is what keeps this an honest "relabel/derive", not new
// data.
const TICKETS_RESOLVED_ID = 'inbox.tickets_resolved'
const AI_DRAFTS_APPROVED_ID = 'inbox.ai_actions_approved'
const AI_DRAFTS_REJECTED_ID = 'inbox.ai_actions_rejected'

export type HeadlineMetricId = 'conversations_handled' | 'ai_draft_acceptance' | 'ai_usage_cost'

// One of the pilot's "three honest numbers on one page" (verevon-feature-map.md
// 1.9 done-enough gate). Each metric resolves its OWN state independently —
// `live` on this metric never implies the other two are live, and it never
// attaches to an unproduced or estimated value.
export type HeadlineMetric = {
  detail: string
  id: HeadlineMetricId
  label: string
  state: MeasurementState
  value: string
}

function scorecardValue(scorecards: readonly InsightScorecard[], id: string): number | null {
  const found = scorecards.find((card) => card.id === id)
  return found ? found.value : null
}

function formatCount(value: number): string {
  return Number.isInteger(value) ? value.toLocaleString('en-US') : value.toFixed(2)
}

function unavailableMetric(id: HeadlineMetricId, label: string): HeadlineMetric {
  return {
    id,
    label,
    state: 'unavailable',
    value: '—',
    detail: 'The measurement layer did not respond before the dashboard timeout. No number is shown rather than a placeholder.',
  }
}

// Cost Core owns the durable AI ledger. It is not attributed to individual
// support resolutions, so this must remain an org-level AI usage cost rather
// than a misleading "cost per resolution" estimate.
function aiUsageCostMetric(cost: ResourceResult<CostSummary>): HeadlineMetric {
  if (cost.state === 'unavailable') {
    return unavailableMetric('ai_usage_cost', 'AI usage cost')
  }

  const entryCount = cost.data.entryCount
  if (entryCount <= 0) {
    return {
      id: 'ai_usage_cost',
      label: 'AI usage cost',
      state: 'empty',
      value: '—',
      detail: 'Cost Core has not recorded any AI usage for this organization yet.',
    }
  }

  return {
    id: 'ai_usage_cost',
    label: 'AI usage cost',
    state: 'live',
    value: `$${cost.data.totalCostUsd.toFixed(4)}`,
    detail: `${formatCount(entryCount)} cost-bearing AI request${entryCount === 1 ? '' : 's'} recorded by Cost Core (${formatCount(cost.data.totalInputTokens)} input / ${formatCount(cost.data.totalOutputTokens)} output tokens).`,
  }
}

// buildHeadlineMetrics derives the pilot's three headline numbers from the
// real overview scorecards. Pure and synchronous so it is unit-testable
// without a network call.
export function buildHeadlineMetrics(
  overview: ResourceResult<InsightScorecard[]>,
  cost: ResourceResult<CostSummary>,
): HeadlineMetric[] {
  if (overview.state === 'unavailable') {
    return [
      unavailableMetric('conversations_handled', 'Conversations handled'),
      unavailableMetric('ai_draft_acceptance', 'AI draft acceptance'),
      aiUsageCostMetric(cost),
    ]
  }

  const scorecards = overview.data
  const resolved = scorecardValue(scorecards, TICKETS_RESOLVED_ID)
  const approved = scorecardValue(scorecards, AI_DRAFTS_APPROVED_ID)
  const rejected = scorecardValue(scorecards, AI_DRAFTS_REJECTED_ID)
  const reviewed = (approved ?? 0) + (rejected ?? 0)

  return [
    {
      id: 'conversations_handled',
      label: 'Conversations handled',
      state: resolved !== null ? 'live' : 'empty',
      value: resolved !== null ? formatCount(resolved) : '—',
      detail: resolved !== null
        ? `Real count of conversation-core tickets marked resolved (${TICKETS_RESOLVED_ID}).`
        : 'No ticket has been marked resolved yet. This populates once conversation-core resolves one.',
    },
    {
      id: 'ai_draft_acceptance',
      label: 'AI draft acceptance',
      state: reviewed > 0 ? 'live' : 'empty',
      value: reviewed > 0 ? `${(((approved ?? 0) / reviewed) * 100).toFixed(1)}%` : '—',
      detail: reviewed > 0
        ? `${formatCount(approved ?? 0)} of ${formatCount(reviewed)} reviewed AI drafts were approved.`
        : 'No AI draft has been approved or rejected yet.',
    },
    aiUsageCostMetric(cost),
  ]
}
