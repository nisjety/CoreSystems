import { z } from 'zod'

// Read-only refresh of the active, session-authorized Insights view. This
// never changes billing settings, runs inference, or starts an Azure import.
export const insightsRefreshActionSchema = z.object({
  type: z.literal('insights.refresh'),
  reason: z.enum(['manual', 'automatic']),
})

export type InsightsRefreshAction = z.infer<typeof insightsRefreshActionSchema>
