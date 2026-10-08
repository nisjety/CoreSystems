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
  summary?: string
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
    detail: 'Temporarily unavailable. Please try refreshing.',
  }
}

// The authenticated Cost Core read is personal usage, not per-case attribution.
function aiUsageCostMetric(cost: ResourceResult<CostSummary>): HeadlineMetric {
  const label = cost.data.scope === 'organization' ? 'Organization AI usage' : 'Your AI usage'
  const audience = cost.data.scope === 'organization' ? 'this organization' : 'your account'
  if (cost.state === 'unavailable') {
    return unavailableMetric('ai_usage_cost', label)
  }

  const summary = cost.data
  const entryCount = summary.entryCount
  if (entryCount <= 0) {
    return {
      id: 'ai_usage_cost',
      label,
      state: 'empty',
      value: '0 requests',
      detail: `No AI usage has been recorded for ${audience} in this period.`,
    }
  }

  if (!summary.basisBreakdownAvailable) {
    return {
      id: 'ai_usage_cost',
      label,
      state: 'partial',
      value: `${formatCount(entryCount)} requests`,
      detail: 'Usage is recorded. Cost details are currently unavailable.',
    }
  }

  const notes = [
    summary.meteredEntryCount > 0
      ? `${formatCount(summary.meteredEntryCount)} metered requests · approximately $${summary.meteredCostUsd.toFixed(2)} estimated API spend`
      : 'No requests explicitly recorded as metered API usage',
    summary.subscriptionEntryCount > 0
      ? `${formatCount(summary.subscriptionEntryCount)} subscription requests · ${formatCount(summary.subscriptionInputTokens + summary.subscriptionOutputTokens)} tokens`
      : '',
    summary.unknownBasisEntryCount > 0
      ? `${formatCount(summary.unknownBasisEntryCount)} entries have unknown billing basis; total spend may be incomplete`
      : '',
    summary.zeroTokenEntryCount > 0
      ? `${formatCount(summary.zeroTokenEntryCount)} entries have no token counts, which does not prove the requests were free`
      : '',
    `${formatCount(summary.totalInputTokens)} input / ${formatCount(summary.totalOutputTokens)} output tokens recorded`,
    summary.latestEntryAt ? `Latest entry ${summary.latestEntryAt.slice(0, 10)}` : '',
  ].filter(Boolean)

  return {
    id: 'ai_usage_cost',
    label,
    state: summary.unknownBasisEntryCount > 0 || summary.zeroTokenEntryCount > 0 ? 'partial' : 'live',
    // Show the known metered subtotal as a subtotal. Unknown-basis rows may
    // include subscription usage or unpriced requests, so never label this as
    // the user's total spend.
    value: `${formatCount(entryCount)} ${entryCount === 1 ? 'request' : 'requests'}`,
    detail: notes.join('. ') + '.',
    summary: summary.unknownBasisEntryCount > 0
      ? 'Usage recorded. Some cost details are unavailable.'
      : summary.zeroTokenEntryCount > 0
        ? 'Usage recorded. Some token measurements are unavailable.'
        : `${formatCount(summary.totalInputTokens + summary.totalOutputTokens)} tokens used in this period.${summary.meteredEntryCount > 0 ? ` API estimate ${summary.meteredCostUsd > 0 && summary.meteredCostUsd < 0.01 ? '< $0.01' : `≈ $${summary.meteredCostUsd.toFixed(2)}`}.` : ''}`,
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
      unavailableMetric('conversations_handled', 'Tickets resolved'),
      unavailableMetric('ai_draft_acceptance', 'AI action approval rate'),
      aiUsageCostMetric(cost),
    ]
  }

  const scorecards = overview.data
  const coverageConfirmed = overview.state === 'live' || overview.state === 'empty'
  const verifiedResolved = scorecards.find((card) => card.id === 'inbox.tickets_currently_resolved' && card.coverage === 'measured')
  const verifiedApproved = scorecards.find((card) => card.id === 'inbox.ai_reviews_approved' && card.coverage === 'measured')
  const verifiedRejected = scorecards.find((card) => card.id === 'inbox.ai_reviews_rejected' && card.coverage === 'measured')
  const verifiedReviews = verifiedApproved && verifiedRejected
  const resolved = scorecardValue(scorecards, TICKETS_RESOLVED_ID)
  const approved = scorecardValue(scorecards, AI_DRAFTS_APPROVED_ID)
  const rejected = scorecardValue(scorecards, AI_DRAFTS_REJECTED_ID)
  // Insight Core omits zero-valued series. A confirmed complete report can
  // therefore treat an absent decision series as zero; partial reports cannot.
  const reviewed = (approved ?? 0) + (rejected ?? 0)

  return [
    {
      id: 'conversations_handled',
      label: verifiedResolved ? 'Resolved tickets' : 'Tickets resolved',
      summary: verifiedResolved ? 'Tickets resolved or closed in this period, excluding reopened tickets.' : 'Recorded resolutions in this period. Coverage may be limited.',
      state: verifiedResolved ? 'live' : !coverageConfirmed ? 'partial' : resolved !== null ? 'live' : 'empty',
      value: verifiedResolved ? formatCount(verifiedResolved.value) : resolved !== null
        ? coverageConfirmed ? formatCount(resolved) : `Observed ${formatCount(resolved)}`
        : '—',
      detail: verifiedResolved ? verifiedResolved.definition : !coverageConfirmed && resolved === null
        ? 'No resolved-ticket events were observed in this period. Source delivery completeness is unknown, so this is not a confirmed zero.'
        : !coverageConfirmed
        ? 'Observed resolved-ticket events from Conversation Core. Source delivery completeness is unknown, so this may not be the full period total.'
        : resolved !== null
        ? `Resolved-ticket events recorded by Conversation Core (${TICKETS_RESOLVED_ID}) during this period.`
        : 'No resolved-ticket events were recorded during this period.',
    },
    {
      id: 'ai_draft_acceptance',
      label: 'AI action approval rate',
      summary: verifiedReviews
        ? (verifiedApproved.value + verifiedRejected.value > 0
          ? `${formatCount(verifiedApproved.value)} approved out of ${formatCount(verifiedApproved.value + verifiedRejected.value)} reviews.`
          : 'No AI actions have been reviewed in this period.')
        : 'Approval rate from recorded reviews. Coverage may be limited.',
      state: verifiedReviews ? ((verifiedApproved.value + verifiedRejected.value) > 0 ? 'live' : 'empty') : !coverageConfirmed ? 'partial' : reviewed > 0 ? 'live' : 'empty',
      value: verifiedReviews ? ((verifiedApproved.value + verifiedRejected.value) > 0
        ? `${((verifiedApproved.value / (verifiedApproved.value + verifiedRejected.value)) * 100).toFixed(1)}%` : '—') : reviewed > 0
        ? `${coverageConfirmed ? '' : 'Observed '}${(((approved ?? 0) / reviewed) * 100).toFixed(1)}%`
        : '—',
      detail: verifiedReviews ? `${formatCount(verifiedApproved.value)} approved / ${formatCount(verifiedApproved.value + verifiedRejected.value)} saved reviews in this period. Verified against Conversation Core; personal scope counts your review decisions.` : !coverageConfirmed && reviewed === 0
        ? 'No approval or rejection events were observed in this period. Source delivery completeness is unknown, so this is not a confirmed zero.'
        : !coverageConfirmed
        ? `${formatCount(approved ?? 0)} of ${formatCount(reviewed)} recorded AI action reviews were approved. This observed rate may be incomplete because source delivery completeness is unknown.`
        : reviewed > 0
        ? `${formatCount(approved ?? 0)} of ${formatCount(reviewed)} recorded AI action reviews were approved; this is not limited to drafts.`
        : 'No AI action approval or rejection events were recorded during this period.',
    },
    aiUsageCostMetric(cost),
  ]
}
