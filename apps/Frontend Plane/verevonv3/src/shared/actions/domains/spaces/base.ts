import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// Space action contracts that predate the Space hybrid build (P-SPA1 moved them
// here from action-registry.ts without changing a line). New Space slices add
// their own fragment next to this one (domains/spaces/<ws>.ts) and one line to
// ./index.ts; the registry itself is not edited again.

const envelopeStatusOutput = z.object({
  status: z.string(),
})

// The spaces.* schemas below all forward to the same domains::spaces route
// handlers the direct REST routes use (apps/gateway/src/domains/spaces.rs,
// exposed to the action dispatcher via pub(crate) rather than proxied a
// second time), so per-room grant checks and the create/bind-then-confirm
// sequence stay in exactly one place.
const spaceCreatePersonalInput = z.object({
  name: z.string().trim().min(1).optional(),
})

const spaceEnsureOrganizationRoomInput = z.object({
  name: z.string().trim().min(1).optional(),
})

const spaceUpdateInstructionsInput = z.object({
  spaceRef: z.string().min(1),
  instructions: z.string().trim().max(4000).optional(),
})

const spaceCreateAgentInput = z.object({
  spaceRef: z.string().min(1),
  name: z.string().trim().min(2).max(60),
  instructions: z.string().trim().max(4000).optional(),
  avatarColor: z
    .string()
    .trim()
    .regex(/^#[0-9a-fA-F]{6}$/, 'avatarColor must be a 6-digit hex color')
    .optional(),
})

const spaceBindAgentInput = z.object({
  spaceRef: z.string().min(1),
  agentRef: z.string().min(1),
})

const spaceRequestPersonalDeletionInput = z.object({
  spaceRef: z.string().min(1),
  idempotencyKey: z.string().trim().min(1).max(160),
})

const spaceUpdateThreadPresentationInput = z
  .object({
    spaceRef: z.string().min(1),
    threadId: z.string().min(1),
    title: z.string().trim().max(200).optional(),
    pinned: z.boolean().optional(),
  })
  .refine((value) => value.title !== undefined || value.pinned !== undefined, {
    message: "Provide either 'title' or 'pinned'",
  })

const spaceMarkReadInput = z.object({
  spaceRef: z.string().min(1),
})

const spaceRecordPresenceInput = z.object({
  spaceRef: z.string().min(1),
  status: z.enum(['online', 'typing', 'offline']).optional(),
})

const spaceSetAgentStateInput = z.object({
  spaceRef: z.string().min(1),
  bindingRef: z.string().min(1),
  status: z.enum(['active', 'paused']),
})

const spaceRevokeAgentInput = z.object({
  spaceRef: z.string().min(1),
  bindingRef: z.string().min(1),
})

// A distinct id from spaces.create_personal_space even though both post to
// the same route and handler (domains/spaces.rs's create_personal_space,
// which dispatches on the body's `kind`): the registered personal-space
// schema has no `kind` field, so it could not express a room create, and a
// room needs a non-empty `name` where a personal Space's is optional.
const spaceCreateRoomInput = z.object({
  kind: z.literal('room'),
  name: z.string().trim().min(1),
})

const spaceAddMemberInput = z.object({
  spaceRef: z.string().min(1),
  memberId: z.string().min(1),
})

const spaceRemoveMemberInput = z.object({
  spaceRef: z.string().min(1),
  memberId: z.string().min(1),
})

export const baseSpaceActions = [
  {
    id: 'spaces.create_personal_space',
    label: 'Create personal Space',
    description: 'Create (or idempotently resolve) the caller\'s own personal Space for the active organization.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceCreatePersonalInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.ensure_organization_room',
    label: 'Ensure organization room',
    description: 'Idempotently provision the organization\'s single shared room, the org-wide channel every member lands in.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceEnsureOrganizationRoomInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.update_space_instructions',
    label: 'Update Space instructions',
    description: 'Update the standing instructions agents in a Space follow. Requires the editor, manager, or owner room role.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceUpdateInstructionsInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.create_space_agent',
    label: 'Create Space agent',
    description: 'Create a new agent definition and grant it a place in this Space. No removal path exists yet, so this is not reversible.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceCreateAgentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.bind_space_agent',
    label: 'Bind existing agent to Space',
    description: 'Grant an org\'s existing agent definition a place in this Space. No removal path exists yet, so this is not reversible.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceBindAgentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.request_personal_space_deletion',
    label: 'Request personal Space deletion',
    description: 'Request deletion of the caller\'s own personal Space. Irreversible once processed.',
    ownerPlane: 'application',
    risk: 'high',
    requiresApproval: true,
    reversible: false,
    inputSchema: spaceRequestPersonalDeletionInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.update_thread_presentation',
    label: 'Update Space thread presentation',
    description: 'Retitle or pin a post in a room. Only the member who started the post may change it.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceUpdateThreadPresentationInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.mark_read',
    label: 'Mark Space read',
    description: 'Advance the caller\'s own read marker for a Space to now.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceMarkReadInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.record_presence',
    label: 'Record Space presence',
    description: '"I am here" — and, in the same answer, who else is. A heartbeat the room already polls on.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceRecordPresenceInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.set_agent_state',
    label: 'Pause or resume Space agent',
    description: 'Pause or resume one bound agent in this room only. Gated to the room\'s owner or manager. Use spaces.revoke_agent to remove it instead.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetAgentStateInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.revoke_agent',
    label: 'Revoke Space agent',
    description: 'Revoke a bound agent from this room. The binding record survives for audit history; reinstating means binding again through the grant flow.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceRevokeAgentInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.create_room',
    label: 'Create room',
    description: 'Create a named shared room. It starts with its creator and grows by explicit grant; it does not inherit the organization\'s roster.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceCreateRoomInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.add_member',
    label: 'Add Space member',
    description: 'Add one person to a named room. Gated to the room\'s owner or manager; the person must already be in the organization.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceAddMemberInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.remove_member',
    label: 'Remove Space member',
    description: 'Remove one person from a named room. The room\'s registered owner cannot be removed this way.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceRemoveMemberInput,
    outputSchema: envelopeStatusOutput,
  },
] as const satisfies readonly ActionDescriptor[]
