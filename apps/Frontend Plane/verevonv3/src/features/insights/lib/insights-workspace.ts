import {
  getInsightsOverview,
  listInsightConnectors,
  type InsightConnector,
  type InsightOverviewQuery,
  type InsightScorecard,
} from '@/shared/api/insights-client'
import { getCostSummary, type CostSummary } from '@/shared/api/cost-client'
import { listResult, withResourceTimeout } from '@/shared/read-data'
import type { MeasurementState, ResourceResult } from '@/shared/read-data'

// Re-export the shared read-data contract so existing Insights consumers keep
// a single vocabulary for live, empty and unavailable states.
export type { MeasurementState, ResourceResult }
export { withResourceTimeout } from '@/shared/read-data'

export type InsightsWorkspace = {
  // Cost Core owns this independent org-level ledger. It is deliberately kept
  // separate from outcome metrics because cost entries do not yet carry a
  // conversation/ticket attribution dimension.
  cost: ResourceResult<CostSummary>
  insightConnectors: ResourceResult<InsightConnector[]>
  overview: ResourceResult<InsightScorecard[]>
}

// Load only authoritative sources for the active tab. The gateway resolves the
// organization from the session; callers can choose a reporting surface/window
// but cannot select or forge a tenant scope.
export async function loadInsightsWorkspace(query: InsightOverviewQuery = {}): Promise<InsightsWorkspace> {
  const [insightConnectors, overview, cost] = await Promise.all([
    withResourceTimeout(readInsightConnectors(), {
      data: [],
      message: 'Insight connector registry did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readInsightsOverview(query), {
      data: [],
      message: 'Metric overview did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
    withResourceTimeout(readCostSummary(), {
      data: emptyCostSummary(),
      message: 'Cost Core did not respond before the dashboard timeout.',
      state: 'unavailable',
    }),
  ])

  return { cost, insightConnectors, overview }
}

// Map real recorded scorecards to a ResourceResult. `listResult` is the
// anti-fabrication helper: rows → `live`, zero rows → `empty`.
export function overviewResult(scorecards: InsightScorecard[]): ResourceResult<InsightScorecard[]> {
  return listResult(scorecards, {
    empty: 'The measurement layer is live, but no matching metric has been recorded in this view yet.',
    live: 'Showing real recorded metrics from the measurement layer.',
  })
}

// Cost Core always returns an honest zeroed rollup for a reachable ledger with
// no matching entries. That is an `empty` measurement, never a real $0 metric.
export function costResult(summary: CostSummary): ResourceResult<CostSummary> {
  return summary.entryCount > 0
    ? {
        data: summary,
        message: 'Showing real Cost Core ledger activity for this organization.',
        state: 'live',
      }
    : {
        data: summary,
        message: 'Cost Core is reachable, but no AI usage has been recorded for this organization yet.',
        state: 'empty',
      }
}

async function readInsightConnectors(): Promise<InsightsWorkspace['insightConnectors']> {
  try {
    const connectors = await listInsightConnectors()
    return {
      data: connectors,
      message: connectors.length
        ? 'Insight Core connector registry is available through the Verevon gateway.'
        : 'Insight Core connector registry is live, but no connectors are registered yet.',
      state: connectors.length ? 'live' : 'empty',
    }
  } catch {
    return {
      data: [],
      message: 'Insight Core connector registry is unavailable.',
      state: 'unavailable',
    }
  }
}

async function readInsightsOverview(query: InsightOverviewQuery): Promise<InsightsWorkspace['overview']> {
  try {
    return overviewResult((await getInsightsOverview(query)).scorecards)
  } catch {
    return {
      data: [],
      message: 'Metric overview is not available; the measurement layer did not respond.',
      state: 'unavailable',
    }
  }
}

async function readCostSummary(): Promise<InsightsWorkspace['cost']> {
  try {
    return costResult(await getCostSummary())
  } catch {
    return {
      data: emptyCostSummary(),
      message: 'Cost Core is not available; no AI usage cost is shown.',
      state: 'unavailable',
    }
  }
}

function emptyCostSummary(): CostSummary {
  return {
    entryCount: 0,
    totalCostUsd: 0,
    totalInputTokens: 0,
    totalOutputTokens: 0,
  }
}
