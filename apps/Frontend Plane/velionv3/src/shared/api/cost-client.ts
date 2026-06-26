import { requestJson } from './http'

// Cost & usage read model for the Cost dashboard (Phase 7 B5). These call the
// gateway `/api/v1/cost/*` proxy (→ cost-core), which resolves the org from the
// session and reshapes cost-core's snake_case rollups to camelCase. Every figure
// is real ledger data: cost-core prices each inference from its catalogue, so an
// org with no activity returns a zeroed summary and an empty entry list.

export type CostSummary = {
  totalInputTokens: number
  totalOutputTokens: number
  totalCostUsd: number
  entryCount: number
}

export type CostEntry = {
  model: string
  runId: string
  requestId: string
  inputTokens: number
  outputTokens: number
  costUsd: number
  createdAt: string
}

export type PricingRate = {
  model: string
  inputPerMillion: number
  outputPerMillion: number
  currency: string
}

const EMPTY_SUMMARY: CostSummary = {
  totalInputTokens: 0,
  totalOutputTokens: 0,
  totalCostUsd: 0,
  entryCount: 0,
}

/** Org-wide rolled-up spend (tokens + USD + entry count). */
export async function getCostSummary(): Promise<CostSummary> {
  const data = await requestJson<Partial<CostSummary> | null>('/api/v1/cost/summary')
  return { ...EMPTY_SUMMARY, ...(data ?? {}) }
}

/** Most-recent cost-bearing entries for the org (newest first). */
export async function listCostEntries(limit = 50): Promise<CostEntry[]> {
  const data = await requestJson<CostEntry[] | null>(
    `/api/v1/cost/entries?limit=${encodeURIComponent(String(limit))}`,
  )
  return Array.isArray(data) ? data : []
}

/** The model price catalogue (USD per 1M tokens) backing the ledger. */
export async function getCostPricing(): Promise<PricingRate[]> {
  const data = await requestJson<PricingRate[] | null>('/api/v1/cost/pricing')
  return Array.isArray(data) ? data : []
}
