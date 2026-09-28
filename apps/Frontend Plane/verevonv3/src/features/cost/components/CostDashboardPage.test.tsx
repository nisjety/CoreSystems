// @vitest-environment jsdom

import { render, waitFor } from '@solidjs/testing-library'
import { describe, expect, it, vi } from 'vitest'
import CostDashboardPage from './CostDashboardPage'

vi.mock('@/shared/api/cost-client', () => ({
  getCostSummary: vi.fn(async () => ({ totalInputTokens: 1500, totalOutputTokens: 400, totalCostUsd: 0.0123, entryCount: 2 })),
  listCostEntries: vi.fn(async () => [
    {
      model: 'gpt-5.6-terra',
      runId: 'run-sub',
      requestId: 'req-sub',
      inputTokens: 1200,
      outputTokens: 300,
      costUsd: 0,
      costBasis: 'subscription',
      createdAt: '2026-09-24T08:00:00Z',
    },
    {
      model: 'gpt-4o-mini',
      runId: 'run-api',
      requestId: 'req-api',
      inputTokens: 300,
      outputTokens: 100,
      costUsd: 0.0123,
      costBasis: 'metered',
      createdAt: '2026-09-24T08:01:00Z',
    },
  ]),
  getCostPricing: vi.fn(async () => []),
}))

describe('CostDashboardPage recent inferences', () => {
  /**
   * A subscription row has real tokens and cost 0. Shown as "$0.00" it would
   * read as free spend, so it says what it is instead.
   */
  it('shows a subscription row as included in the subscription, not as $0.00', async () => {
    const { container, unmount } = render(() => <CostDashboardPage />)
    await waitFor(() => expect(container.querySelectorAll('.verevon-cost__table tbody tr')).toHaveLength(2))
    const costs = Array.from(container.querySelectorAll('.verevon-cost__table tbody .verevon-cost__cost')).map(
      (cell) => cell.textContent?.trim(),
    )
    expect(costs[0]).toMatch(/^(Inkludert i abonnementet|Included in the subscription)$/)
    expect(costs[1]).toBe('$0.0123')
    unmount()
  })
})
