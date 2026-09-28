import { z } from 'zod'

// Personal presentation contracts for the reversible browser-only organizer.
// These never grant access, change a thread's server metadata or invoke a model.
const id = z.string().trim().min(1).max(200)
const name = z.string().trim().min(1).max(80)
export const chatOrganizationViewSchema = z.object({
  sort: z.enum(['recent', 'oldest', 'title']).default('recent'),
  group: z.enum(['project', 'date', 'none']).default('project'),
  filter: z.enum(['all', 'pinned', 'active', 'attention']).default('all'),
})
export const chatOrganizationSchema = z.object({
  version: z.literal(1),
  projects: z.array(z.object({ id, name, pinned: z.boolean().default(false) })).max(100),
  assignments: z.record(z.string(), z.string()),
  view: chatOrganizationViewSchema,
})
export const chatOrganizationActionSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('project.create'), id, name }),
  z.object({ type: z.literal('project.rename'), id, name }),
  z.object({ type: z.literal('project.remove'), id }),
  z.object({ type: z.literal('project.pin'), id, pinned: z.boolean() }),
  z.object({ type: z.literal('thread.move'), threadId: id, projectId: id.nullable() }),
  z.object({ type: z.literal('view.update'), view: chatOrganizationViewSchema }),
])
export type ChatOrganization = z.infer<typeof chatOrganizationSchema>
export type ChatOrganizationAction = z.infer<typeof chatOrganizationActionSchema>
