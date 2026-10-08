import { requestJson } from './http'
import { z } from 'zod'

const usageBucketSchema = z.object({
  day: z.string().regex(/^\d{4}-\d{2}-\d{2}$/),
  model: z.string(), provider: z.string(), basis: z.enum(['metered', 'subscription', 'unknown']),
  requests: z.number().int().nonnegative(), input_tokens: z.number().int().nonnegative(),
  output_tokens: z.number().int().nonnegative(), metered_cost_usd: z.number().finite().nonnegative(),
  unmeasured_requests: z.number().int().nonnegative(),
})
const usageAnalyticsSchema = z.object({
  buckets: z.array(usageBucketSchema), scope: z.enum(['me', 'organization']),
  reporting_started_at: z.string().datetime({ offset: true }).nullable(),
  from: z.string().datetime({ offset: true }).nullable(), to: z.string().datetime({ offset: true }),
  timezone: z.literal('UTC'),
})
export type UsageBucket = z.infer<typeof usageBucketSchema>
export type UsageAnalytics = z.infer<typeof usageAnalyticsSchema>

export async function getUsageAnalytics(query: CostSummaryQuery): Promise<UsageAnalytics> {
  const params = new URLSearchParams({ scope: query.scope ?? 'me' })
  if (query.from) params.set('from', query.from)
  if (query.to) params.set('to', query.to)
  return usageAnalyticsSchema.parse(await requestJson(`/api/v1/cost/analytics?${params}`))
}

// Cost & usage read model. Personal reads are actor-pinned; explicit org reads
// require owner/admin authority signed by Auth Core. The active org is always
// resolved server-side, with subscription, metered and unknown bases separate.

export type CostSummary = {
  reportingStartedAt?: string | null
  scope?: 'organization' | 'me'
  totalInputTokens: number
  totalOutputTokens: number
  totalCostUsd: number
  entryCount: number
  meteredEntryCount: number
  subscriptionEntryCount: number
  unknownBasisEntryCount: number
  meteredCostUsd: number
  subscriptionInputTokens: number
  subscriptionOutputTokens: number
  unknownBasisInputTokens: number
  unknownBasisOutputTokens: number
  zeroTokenEntryCount: number
  latestEntryAt: string | null
  basisBreakdownAvailable: boolean
}

export type CostSummaryQuery = { from?: string; to?: string; scope?: 'organization' | 'me' }

export type UserCostUsage = {
  user_id: string
  entry_count: number
  input_tokens: number
  output_tokens: number
  metered_cost_usd: number
  metered_entries: number
  subscription_entries: number
  unknown_basis_entries: number
  unmeasured_token_entries: number
}

/** All entries in the window, grouped by user; owners/admins only. */
export async function getUserCostUsage(query: Pick<CostSummaryQuery, 'from' | 'to'>): Promise<UserCostUsage[]> {
  const params = new URLSearchParams()
  if (query.from) params.set('from', query.from)
  if (query.to) params.set('to', query.to)
  const data = await requestJson<unknown>(`/api/v1/cost/users${params.size ? `?${params}` : ''}`)
  if (!Array.isArray(data)) throw new Error('Invalid user usage report')
  for (const row of data) {
    if (!row || typeof row.user_id !== 'string' ||
      !['entry_count', 'input_tokens', 'output_tokens', 'metered_cost_usd', 'metered_entries', 'subscription_entries', 'unknown_basis_entries', 'unmeasured_token_entries']
        .every((key) => typeof row[key] === 'number' && Number.isFinite(row[key]) && row[key] >= 0)) {
      throw new Error('Invalid user usage measurement')
    }
  }
  return data as UserCostUsage[]
}

export type AzureActualCostSummary = {
  available: boolean
  rows: number
  byCurrency: Array<{ currency: string; amount: number }>
  latestUsageDate: string | null
}

export type CostEntry = {
  model: string
  runId: string
  requestId: string
  inputTokens: number
  outputTokens: number
  costUsd: number
  /**
   * How the cost was reached. A `subscription` row (the user's own ChatGPT
   * subscription) has real tokens and cost 0, which is not free spend. `null`
   * when the producer did not say.
   */
  costBasis?: 'metered' | 'subscription' | null
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

/** Personal by default; explicit organization scope requires owner/admin membership. */
export async function getCostSummary(query: CostSummaryQuery = {}): Promise<CostSummary> {
  const params = new URLSearchParams()
  if (query.scope) params.set('scope', query.scope)
  if (query.from) params.set('from', query.from)
  if (query.to) params.set('to', query.to)
  const suffix = params.size ? `?${params.toString()}` : ''
  const data = await requestJson<Partial<CostSummary> | null>(`/api/v1/cost/summary${suffix}`)
  return { ...EMPTY_SUMMARY, ...(data ?? {}), basisBreakdownAvailable: data?.basisBreakdownAvailable === true }
}

/** Azure Cost Management actual charges, aggregated at organization scope. */
export async function getAzureActualCostSummary(query: CostSummaryQuery = {}): Promise<AzureActualCostSummary> {
  const params = new URLSearchParams()
  if (query.from) params.set('from', query.from)
  if (query.to) params.set('to', query.to)
  const suffix = params.size ? `?${params.toString()}` : ''
  const data = await requestJson<Partial<AzureActualCostSummary> | null>(`/api/v1/cost/azure/actual${suffix}`)
  return {
    available: data?.available === true,
    rows: Number.isFinite(data?.rows) ? Number(data?.rows) : 0,
    byCurrency: Array.isArray(data?.byCurrency) ? data.byCurrency.filter((item) =>
      typeof item?.currency === 'string' && Number.isFinite(item.amount),
    ) : [],
    latestUsageDate: typeof data?.latestUsageDate === 'string' ? data.latestUsageDate : null,
  }
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
