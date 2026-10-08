import { z } from 'zod'

// Local report export and read only: no payment or credit mutation.
export const insightsUsageActionSchema = z.object({
  type: z.enum(['insights.usage.read', 'insights.usage.export']),
  scope: z.enum(['organization', 'me']),
  from: z.string().datetime({ offset: true }).optional(),
  to: z.string().datetime({ offset: true }).optional(),
})
