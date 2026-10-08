import {
  getInsightsOverview,
  type InsightOverviewQuery,
  type InsightScorecard,
} from '@/shared/api/insights-client'
import { getAzureActualCostSummary, getCostSummary, type AzureActualCostSummary, type CostSummary } from '@/shared/api/cost-client'
import { ApiError } from '@/shared/api/http'
import { listResult, withResourceTimeout } from '@/shared/read-data'
import type { MeasurementState, ResourceResult } from '@/shared/read-data'

// Re-export the shared read-data contract so existing Insights consumers keep
// a single vocabulary for live, empty and unavailable states.
export type { MeasurementState, ResourceResult }
export { withResourceTimeout } from '@/shared/read-data'

export type InsightsWorkspace = {
  // Cost Core resolves this to the authenticated user or authorized org ledger. It is separate
  // from organization-level outcomes and is never represented as cost per case.
  cost: ResourceResult<CostSummary>
  azureActualCost: ResourceResult<AzureActualCostSummary>
  overview: ResourceResult<InsightScorecard[]>
}

// Load only authoritative sources for the active tab. The gateway resolves the
// organization from the session; callers can choose a reporting surface/window
// but cannot select or forge a tenant scope.
export async function loadInsightsWorkspace(
  query: InsightOverviewQuery = {},
  options: { organizationCostAccess?: boolean } = {},
): Promise<InsightsWorkspace> {
  const costScope = options.organizationCostAccess && query.scope !== 'me' ? 'organization' : 'me'
  const [overview, cost, azureActualCost] = await Promise.all([
    withResourceTimeout(readInsightsOverview(query), {
      data: [],
      message: 'Metric overview did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readCostSummary(query, costScope), {
      data: emptyCostSummary(costScope),
      message: 'Cost Core did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(costScope === 'organization' ? readAzureActualCostSummary(query) : Promise.resolve({
      data: emptyAzureActualCostSummary(),
      message: 'Organization billing data is available to organization owners and admins.',
      state: 'not_connected' as const,
    }), {
      data: emptyAzureActualCostSummary(),
      message: 'Azure actual cost data did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
  ])

  return { cost, overview, azureActualCost }
}

// Map real recorded scorecards to a ResourceResult. `listResult` is the
// anti-fabrication helper: rows → `live`, zero rows → `empty`.
export function overviewResult(
  scorecards: InsightScorecard[],
  coverage: 'unknown' | 'measured' = 'measured',
  coverageDetail = '',
): ResourceResult<InsightScorecard[]> {
  if (coverage === 'unknown') {
    return {
      data: scorecards,
      message: coverageDetail || (scorecards.length > 0
        ? 'Showing recorded source events. The source has not yet reported delivery checkpoints, so completeness cannot be confirmed.'
        : 'No matching rows were returned, but source delivery checkpoints are unavailable, so this cannot be interpreted as zero activity.'),
      state: 'partial',
    }
  }
  return listResult(scorecards, {
    empty: 'The measurement layer is live, but no matching metric has been recorded in this view yet.',
    live: 'Showing real recorded metrics from the measurement layer.',
  })
}

// Cost Core always returns an honest zeroed rollup for a reachable ledger with
// no matching entries. That is an `empty` measurement, never a real $0 metric.
export function costResult(summary: CostSummary): ResourceResult<CostSummary> {
  const audience = summary.scope === 'organization' ? 'this organization' : 'your account'
  if (summary.entryCount <= 0) {
    return {
      data: summary,
      message: `Cost Core is reachable, but no AI usage has been recorded for ${audience} in this period.`,
      state: 'empty',
    }
  }
  return summary.basisBreakdownAvailable && summary.unknownBasisEntryCount === 0 && summary.zeroTokenEntryCount === 0
    ? {
        data: summary,
        message: `Showing recorded Cost Core activity for ${audience} in this period.`,
        state: 'live',
      }
    : {
        data: summary,
        message: !summary.basisBreakdownAvailable
          ? 'Usage was recorded, but Cost Core has not provided the cost-basis breakdown needed to verify metered spend.'
          : summary.unknownBasisEntryCount > 0
            ? 'Some entries have an unknown billing basis, so the usage breakdown is partial.'
            : 'Some entries have no token measurements; a zero-token entry does not prove that the request used no tokens.',
        state: 'partial',
      }
}

async function readInsightsOverview(query: InsightOverviewQuery): Promise<InsightsWorkspace['overview']> {
  try {
    const report = await getInsightsOverview(query)
    return overviewResult(report.scorecards, report.coverage, report.coverageDetail)
  } catch {
    return {
      data: [],
      message: 'Metric overview is not available; the measurement layer did not respond.',
      state: 'unavailable',
    }
  }
}

async function readCostSummary(query: InsightOverviewQuery, scope: 'organization' | 'me'): Promise<InsightsWorkspace['cost']> {
  try {
    return costResult(await getCostSummary({ from: query.from, to: query.to, scope }))
  } catch {
    return {
      data: emptyCostSummary(scope),
      message: 'Cost Core is not available; no AI usage cost is shown.',
      state: 'unavailable',
    }
  }
}

async function readAzureActualCostSummary(query: InsightOverviewQuery): Promise<InsightsWorkspace['azureActualCost']> {
  try {
    const summary = await getAzureActualCostSummary({ from: query.from, to: query.to })
    return azureActualCostResult(summary)
  } catch (error) {
    return {
      data: emptyAzureActualCostSummary(),
      message: error instanceof ApiError && error.status === 403
        ? 'Only an organization owner or admin can view Azure actual charges.'
        : 'Azure Cost Management actual charges are unavailable; no amount is inferred from token usage.',
      state: 'unavailable',
    }
  }
}

// Azure rows are exposed only after Cost Core maps their project to this org.
// An empty result therefore means no mapped rows, not that shared/untagged
// Azure spend is zero or safe to attribute to this tenant.
export function azureActualCostResult(summary: AzureActualCostSummary): ResourceResult<AzureActualCostSummary> {
  if (!summary.available) {
    return { data: summary, message: 'Azure Cost Management is not configured for this organization.', state: 'unavailable' }
  }
  if (summary.rows === 0) {
    return {
      data: summary,
      message: 'No Azure actual charges are currently mapped to this organization for this period. Shared and untagged Azure charges are kept out of tenant totals.',
      state: 'empty',
    }
  }
  return {
    data: summary,
    message: 'Showing Azure actual charges for projects explicitly mapped to this organization. Azure billing can be delayed and untagged projects are excluded.',
    state: 'live',
  }
}

function emptyAzureActualCostSummary(): AzureActualCostSummary {
  return { available: false, rows: 0, byCurrency: [], latestUsageDate: null }
}

function emptyCostSummary(scope: 'organization' | 'me'): CostSummary {
  return {
    scope,
    entryCount: 0,
    totalCostUsd: 0,
    totalInputTokens: 0,
    totalOutputTokens: 0,
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
    basisBreakdownAvailable: false,
  }
}
