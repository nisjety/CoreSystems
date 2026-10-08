import { beforeEach, describe, expect, it, vi } from 'vitest'
import {
  costResult,
  loadInsightsWorkspace,
  azureActualCostResult,
  overviewResult,
  withResourceTimeout,
  type ResourceResult,
} from '@/features/insights/lib/insights-workspace'
import type { InsightScorecard } from '@/shared/api/insights-client'
import type { AzureActualCostSummary } from '@/shared/api/cost-client'
import { getAzureActualCostSummary, getCostSummary } from '@/shared/api/cost-client'
import { getInsightsOverview } from '@/shared/api/insights-client'

vi.mock('@/shared/api/cost-client', () => ({ getAzureActualCostSummary: vi.fn(), getCostSummary: vi.fn() }))
vi.mock('@/shared/api/insights-client', () => ({ getInsightsOverview: vi.fn() }))

describe('workspace cost scope', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(getInsightsOverview).mockResolvedValue({ scorecards: [], coverage: 'unknown', coverageDetail: '', contractVersion: 'test', generatedAt: '', window: {}, sourceCount: 0 })
    vi.mocked(getCostSummary).mockResolvedValue({ entryCount: 0 } as Awaited<ReturnType<typeof getCostSummary>>)
    vi.mocked(getAzureActualCostSummary).mockResolvedValue({ available: true, rows: 0, byCurrency: [], latestUsageDate: null })
  })

  it('keeps member cost reads personal and never requests restricted Azure data', async () => {
    await loadInsightsWorkspace({ scope: 'organization' })
    expect(getCostSummary).toHaveBeenCalledWith(expect.objectContaining({ scope: 'me' }))
    expect(getAzureActualCostSummary).not.toHaveBeenCalled()
  })

  it('requests the authorized organization ledger and Azure actuals for owners/admins', async () => {
    await loadInsightsWorkspace({ scope: 'organization' }, { organizationCostAccess: true })
    expect(getCostSummary).toHaveBeenCalledWith(expect.objectContaining({ scope: 'organization' }))
    expect(getAzureActualCostSummary).toHaveBeenCalledOnce()
  })

  it('keeps an admin personal view personal and preserves its scope on failure', async () => {
    vi.mocked(getCostSummary).mockRejectedValue(new Error('unavailable'))
    const workspace = await loadInsightsWorkspace({ scope: 'me' }, { organizationCostAccess: true })
    expect(workspace.cost.state).toBe('unavailable')
    expect(workspace.cost.data.scope).toBe('me')
    expect(getAzureActualCostSummary).not.toHaveBeenCalled()
  })
})

describe('overviewResult', () => {
  it('maps recorded scorecards to a live result that carries the real rows', () => {
    const scorecards: InsightScorecard[] = [
      { id: 'inbox.a', label: 'Inbox a', definition: 'Recorded source events.', metric: 'a', surface: 'inbox', unit: 'count', value: 4, source: 'conversation-core', eventCount: 2, aggregation: 'sum_of_recorded_events' },
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
    const result = costResult({
      entryCount: 2, totalCostUsd: 0.004, totalInputTokens: 100, totalOutputTokens: 20,
      meteredEntryCount: 2, subscriptionEntryCount: 0, unknownBasisEntryCount: 0,
      meteredCostUsd: 0.004, subscriptionInputTokens: 0, subscriptionOutputTokens: 0,
      unknownBasisInputTokens: 0, unknownBasisOutputTokens: 0, zeroTokenEntryCount: 0,
      latestEntryAt: null, basisBreakdownAvailable: true,
    })

    expect(result.state).toBe('live')
    expect(result.data.totalCostUsd).toBe(0.004)
  })

  it('marks a zeroed cost-core rollup as empty instead of showing a fabricated $0 metric', () => {
    const result = costResult({
      entryCount: 0, totalCostUsd: 0, totalInputTokens: 0, totalOutputTokens: 0,
      meteredEntryCount: 0, subscriptionEntryCount: 0, unknownBasisEntryCount: 0,
      meteredCostUsd: 0, subscriptionInputTokens: 0, subscriptionOutputTokens: 0,
      unknownBasisInputTokens: 0, unknownBasisOutputTokens: 0, zeroTokenEntryCount: 0,
      latestEntryAt: null, basisBreakdownAvailable: true,
    })

    expect(result.state).toBe('empty')
  })
})

describe('azureActualCostResult', () => {
  it('explains that empty tenant totals exclude shared and untagged Azure charges', () => {
    const summary: AzureActualCostSummary = { available: true, rows: 0, byCurrency: [], latestUsageDate: null }

    const result = azureActualCostResult(summary)

    expect(result.state).toBe('empty')
    expect(result.message).toContain('Shared and untagged Azure charges are kept out of tenant totals.')
  })

  it('shows only the explicit tenant-mapped Azure totals', () => {
    const summary: AzureActualCostSummary = {
      available: true,
      rows: 2,
      byCurrency: [{ currency: 'NOK', amount: 42.5 }],
      latestUsageDate: '2026-10-03',
    }

    const result = azureActualCostResult(summary)

    expect(result.state).toBe('live')
    expect(result.data.byCurrency).toEqual([{ currency: 'NOK', amount: 42.5 }])
    expect(result.message).toContain('projects explicitly mapped to this organization')
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
