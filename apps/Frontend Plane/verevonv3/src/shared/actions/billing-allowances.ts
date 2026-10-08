import { z } from 'zod'

export const billingAllowancesActionSchema = z.object({
  type: z.literal('billing.allowances.read'),
  scope: z.literal('organization'),
})
