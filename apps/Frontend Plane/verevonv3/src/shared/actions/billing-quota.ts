import { z } from 'zod'

// The member's organisation allowance for one billing metric, read through the BFF route
// `GET /api/v1/billing/quotas/{metric}`. The organisation comes from the verified session, so it is never
// an input here; the schema is strict, so a client that names one is refused rather than silently dropped.
export const billingQuotaActionSchema = z
  .object({
    type: z.literal('billing.quota.read'),
    metric: z.string().trim().min(1).max(128),
    scope: z.literal('organization'),
  })
  .strict()
