import { z } from 'zod'
import { requestJson } from './http'

const units = z.number().int().nonnegative().safe()
const date = z.string().datetime({ offset: true })
const period = z.object({ from: date, to: date, included: units, carried: units, granted: units, used: units, overage: units, expired: units, remaining: units })
export const allowanceSchema = z.object({
  policy: z.object({ metric: z.string(), name: z.string(), unit: z.enum(['tokens', 'requests', 'units']), version: z.string(), starts_at: date, ends_at: date.optional(), interval_months: z.number().int().positive(), included_units: units, rollover_cap: units, rollover_cycles: z.number().int() }),
  current: period, history: z.array(period), as_of: date, next_expiry: date.optional(), active: z.boolean(),
  next_change: z.object({ version: z.string(), effective_at: date, included_units: units, rollover_cap: units, rollover_cycles: z.number().int(), reason: z.string() }).optional(),
})
export type BillingAllowance = z.infer<typeof allowanceSchema>
export async function getBillingAllowances(): Promise<BillingAllowance[]> {
  return z.array(allowanceSchema).parse(await requestJson('/api/v1/billing/allowances'))
}
