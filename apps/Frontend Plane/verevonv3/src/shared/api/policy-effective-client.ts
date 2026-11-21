import { requestJson } from './http'
import { z } from 'zod'

// The effective policy of one thread (Wave 3 EP-9 T2), as the browser's aggregate route returns it. Each value names
// the layer that set it, and `locked` is set when a higher layer fixes it. A value an owner did not answer for is
// `unavailable` with no value, so a caller shows that state and never a guess in its place. A member's items carry no
// `revision`; the revision is admin detail.
const effectivePolicyItemSchema = z.object({
  key: z.string(),
  value: z.unknown(),
  origin: z.enum(['platform', 'org', 'space', 'user', 'request']).nullable(),
  revision: z.number().int().nonnegative().optional(),
  locked: z.boolean(),
  unavailable: z.boolean().default(false),
  // A list a member may not read comes as its count alone (`redacted`); an admin reads its entries as text. The
  // panel reads the same two shapes, so the two readers agree on what a list is.
  redacted: z.boolean().optional(),
  count: z.number().int().nonnegative().optional(),
  // Each entry is non-blank text of at most 200 characters, counted in code points as the panel and server do.
  values: z
    .array(z.string().refine((entry) => entry.trim() !== '' && [...entry].length <= 200))
    .max(200)
    .optional(),
})

// requestJson has already unwrapped the route's `{ data }` envelope.
const effectivePolicyBodySchema = z.object({
  items: z.array(effectivePolicyItemSchema),
})

export type EffectivePolicyItem = z.infer<typeof effectivePolicyItemSchema>

/** The effective policy of one thread, value by value, in the order the server lists them. */
export async function getEffectivePolicy(threadId: string): Promise<EffectivePolicyItem[]> {
  const params = new URLSearchParams({ thread_id: threadId })
  return effectivePolicyBodySchema.parse(await requestJson(`/api/v1/policy/effective?${params}`)).items
}
