import { z } from 'zod'

// Reads the server-authorized organization report; it is not a user/tenant
// delegation grant and does not change billing or allocate Azure invoices.
export const insightsUserReportActionSchema = z.object({
  type: z.literal('insights.users.read'),
  from: z.string().datetime({ offset: true }).optional(),
  to: z.string().datetime({ offset: true }).optional(),
})
