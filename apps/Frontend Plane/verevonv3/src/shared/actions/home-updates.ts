import { z } from 'zod'

/** Presentation-only feed selection; each read still uses its owning API. */
export const homeUpdatesFeedSchema = z.enum(['news', 'traffic', 'aquatiq'])
export type HomeUpdatesFeed = z.infer<typeof homeUpdatesFeedSchema>
export const homeUpdatesActionSchema = z.object({
  type: z.literal('home.updates.select'),
  feed: homeUpdatesFeedSchema,
})
