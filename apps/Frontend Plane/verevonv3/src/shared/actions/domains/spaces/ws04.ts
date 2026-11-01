import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS04-S6: room member writer contracts. Control (user-core `internal/spaces`) owns the role
// matrix and decides every call under the caller's signed delegation; the gateway only proxies
// (apps/gateway/src/domains/spaces/room_admin.rs, dispatchers_spaces/ws04.rs). Neither id is
// model-executable: who may read a room's shared record is a human decision
// (model-access.ts closes both, model-eligibility.ts never lists them).

const envelopeStatusOutput = z.object({
  status: z.string(),
})

const roomRole = z.enum(['viewer', 'editor', 'manager', 'owner'])

const spaceChangeMemberRoleInput = z.object({
  spaceRef: z.string().min(1),
  memberId: z.string().min(1),
  role: roomRole,
  // Optimistic concurrency: the membership revision the caller saw. A stale one answers
  // 409 `room_member_changed` with `details.current_revision`.
  expectedRevision: z.number().int().min(0).optional(),
})

const spaceLeaveRoomInput = z.object({
  spaceRef: z.string().min(1),
})

// WS04-S2: a room's name and topic. Application (convex-core `spaces`) owns the labels and
// runs the one set of rules (NFC, trim, name 1-120, topic 0-250, no control or bidi
// characters); the gateway only gates owner or manager from Control's live role. The topic is
// a display label, never prompt or notification content. An empty `topic` clears it.
const spaceUpdateRoomDetailsInput = z
  .object({
    spaceRef: z.string().min(1),
    name: z.string().max(2000).optional(),
    topic: z.string().max(2000).optional(),
  })
  .refine((value) => value.name !== undefined || value.topic !== undefined, {
    message: 'Provide a name or a topic.',
  })

const spaceUpdateRoomDetailsOutput = z.object({
  space_ref: z.string(),
  name: z.string(),
  topic: z.string().nullable(),
})

export const ws04SpaceActions = [
  {
    id: 'spaces.update_room_details',
    label: 'Update Space room name and topic',
    description:
      'Rename a named room or set its topic. Only a room owner or manager may; the organization room and personal Spaces follow their own rules. An empty topic clears it.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceUpdateRoomDetailsInput,
    outputSchema: spaceUpdateRoomDetailsOutput,
  },
  {
    id: 'spaces.change_member_role',
    label: 'Change Space member role',
    description:
      'Change one person\'s role in a named room (viewer, editor, manager, owner). Control enforces the role matrix: a manager cannot grant or change manager or owner, nobody promotes themselves, and the last owner cannot be demoted.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceChangeMemberRoleInput,
    outputSchema: envelopeStatusOutput,
  },
  {
    id: 'spaces.leave_room',
    label: 'Leave Space room',
    description:
      'Leave a named room you are a member of. The last owner cannot leave; the organization room and personal Spaces are refused.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceLeaveRoomInput,
    outputSchema: envelopeStatusOutput,
  },
] as const satisfies readonly ActionDescriptor[]
