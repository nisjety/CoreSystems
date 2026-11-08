import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS08-S5: the binding wake policy. Application (convex-core `spaceAgents`) stores `respondTo`
// and records every change in `spaceAgentPolicyEvents`; the gateway gates owner or manager from
// Control's live role (apps/gateway/src/domains/spaces/agent_policy.rs, dispatchers_spaces/ws08.rs).
// The id is not model-executable (model-access.ts closes it): who may wake an agent is a room
// governance decision, and a policy never grants anything, Control still decides who may post.

const spaceSetAgentWakePolicyInput = z
  .object({
    spaceRef: z.string().min(1),
    bindingRef: z.string().min(1),
    // `members` = any editor, manager or owner. `group` is retired and is not a value.
    respondTo: z.enum(['members', 'managers', 'allowlist']),
    // Control subject ids, at most 50; required for `allowlist`, refused for the other modes.
    respondToSubjects: z.array(z.string().min(1).max(128)).max(50).optional(),
    // Optimistic concurrency: the binding `updated_at` the caller saw. A stale one answers 409
    // `policy_conflict`.
    expectedUpdatedAt: z.number().int().min(0),
  })
  .refine(
    (value) =>
      value.respondTo === 'allowlist'
        ? (value.respondToSubjects?.length ?? 0) > 0
        : (value.respondToSubjects?.length ?? 0) === 0,
    { message: 'Subjects belong to, and are required by, the allowlist mode only.' },
  )

const spaceSetAgentWakePolicyOutput = z.object({
  binding_ref: z.string(),
  respond_to: z.enum(['members', 'managers', 'allowlist']),
  respond_to_subjects: z.array(z.string()),
  projection_version: z.number().nullable(),
  updated_at: z.number().nullable(),
  changed: z.boolean(),
})

export const ws08SpaceActions = [
  {
    id: 'spaces.set_agent_wake_policy',
    label: 'Set who can wake a Space agent',
    description:
      'Choose who may wake one bound agent in this room: any editor, manager or owner (members), managers and owners only, or a named allowlist. Only the room\'s owner or manager may change it. It narrows or widens the mention path only; it never grants a right Control withholds.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetAgentWakePolicyInput,
    outputSchema: spaceSetAgentWakePolicyOutput,
  },
] as const satisfies readonly ActionDescriptor[]
