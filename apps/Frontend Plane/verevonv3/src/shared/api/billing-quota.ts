import { z } from 'zod'
import { billingQuotaActionSchema } from '@/shared/actions/billing-quota'
import { requestJson } from './http'

// billing-core's QuotaStatus. The amounts belong to the organisation; the readout shows only the share.
export const quotaStatusSchema = z.object({
  limit: z.number().nonnegative(),
  used: z.number().nonnegative(),
  remaining: z.number().nonnegative(),
  is_exceeded: z.boolean(),
  utilization: z.number().min(0).max(1),
})

type BillingQuotaStatus = z.infer<typeof quotaStatusSchema>

/**
 * The billing metric the member's readout reads, from `VITE_QUOTA_READOUT_METRIC`. Undefined unless
 * `VITE_QUOTA_READOUT` is the literal `true` and names a metric, so an unset build reads nothing. The metric is
 * a build-time setting, one per deployment (WS27-D9).
 */
export function quotaReadoutMetric(): string | undefined {
  if (import.meta.env.VITE_QUOTA_READOUT !== 'true') return undefined
  const metric = String(import.meta.env.VITE_QUOTA_READOUT_METRIC ?? '').trim()
  return metric === '' ? undefined : metric
}

export async function getBillingQuota(metric: string): Promise<BillingQuotaStatus> {
  const input = billingQuotaActionSchema.parse({ type: 'billing.quota.read', metric, scope: 'organization' })
  return quotaStatusSchema.parse(await requestJson(`/api/v1/billing/quotas/${encodeURIComponent(input.metric)}`))
}
