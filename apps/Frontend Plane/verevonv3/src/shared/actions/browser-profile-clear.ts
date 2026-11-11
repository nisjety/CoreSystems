import { z } from 'zod'

// Presentation-level contract for clearing a stored browser profile's browsing data
// (F-t2-068). It is a standalone contract, not a registry row: clearing is human-only
// and never model-visible, so the Model Plane admission golden does not change. The
// edge serves the operation only when QUARRY_EDGE__PROFILE_CLEAR_DATA is on (the profile
// list then says clear_data_enabled), and it keeps its own authorization and in-use
// checks. A dry run is the default; only an explicit confirm can remove data.
export const browserProfileClearActionSchema = z.object({
  type: z.literal('browser.profile.clear_data'),
  profileId: z.string().regex(/^prof_[A-Za-z0-9_-]+$/),
  dryRun: z.boolean(),
})
export type BrowserProfileClearAction = z.infer<typeof browserProfileClearActionSchema>

// `confirmed` false is the count-first preview (dryRun true). Only `confirmed` true asks
// for a live clear. An id the schema rejects throws here, before any request is sent.
export function clearDataStep(profileId: string, confirmed: boolean): BrowserProfileClearAction {
  return browserProfileClearActionSchema.parse({
    type: 'browser.profile.clear_data',
    profileId,
    dryRun: !confirmed,
  })
}
