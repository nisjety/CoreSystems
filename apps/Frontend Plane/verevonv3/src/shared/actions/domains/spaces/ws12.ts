import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS12-S3: direct messages. `spaces.open_dm` opens (or returns) the DM for exactly one set of
// people; Control decides whether the organization may have DMs and whether these people can be
// in one (`dm-eligibility`), Application stores the Space record, and Control registers the fixed
// roster afterwards (apps/gateway/src/domains/spaces/dm.rs, dispatchers_spaces/ws12.rs).
// `spaces.hide_space` hides or shows one DM in the caller's own sidebar (presentation only, ids
// and a time). Neither id is model-executable (model-access.ts closes both). A Zero Data
// Retention organization is refused `spaces.open_dm` with the typed
// `zdr_durable_mutation_forbidden` before any Control or Application call.

// The OTHER people (the caller is added from the session): 1 to 8 distinct identity ids, so a DM
// holds 2 people and a group DM 3 to 9.
const spaceOpenDmInput = z.object({
  participants: z.array(z.string().trim().min(1).max(128)).min(1).max(8),
  clientRequestId: z.string().trim().min(1).max(200),
})

const spaceOpenDmOutput = z.object({
  space: z.object({
    space_ref: z.string(),
    name: z.string(),
    kind: z.enum(['dm', 'group_dm']),
    lifecycle: z.string(),
    participants: z.array(z.object({ subject_id: z.string() })),
  }),
  created: z.boolean(),
})

const spaceHideSpaceInput = z.object({
  spaceRef: z.string().min(1),
  // Absent hides; false shows the DM again.
  hidden: z.boolean().optional(),
})

const spaceHideSpaceOutput = z.object({
  space_ref: z.string(),
  hidden: z.boolean(),
  hidden_at: z.number().nullable(),
})

export const ws12SpaceActions = [
  {
    id: 'spaces.open_dm',
    label: 'Open direct message',
    description:
      'Open a direct message with one person, or a group direct message with up to eight, or return the one that already exists for exactly those people. The people cannot be changed afterwards, and Control decides whether your organization may have direct messages.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceOpenDmInput,
    outputSchema: spaceOpenDmOutput,
  },
  {
    id: 'spaces.hide_space',
    label: 'Hide direct message',
    description:
      'Hide one of your direct messages from your own sidebar, or show it again. Nothing is deleted and the other people are not affected; a new message brings it back.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceHideSpaceInput,
    outputSchema: spaceHideSpaceOutput,
  },
] as const satisfies readonly ActionDescriptor[]
