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

// WS04-S8: room visibility, the self-join role and joining an open room. Control (user-core
// `internal/spaces`) owns visibility, join role, the per-org directory policy and the join
// itself; the gateway proxies (domains/spaces/room_admin.rs, dispatchers_spaces/ws04.rs) and
// reconciles the recipient audience after a join. Neither id is model-executable
// (model-access.ts closes both): who may enter a room or open it to the organization is a
// human decision.
const roomVisibility = z.enum(['private', 'org_public'])
// A self-join never receives manager or owner: Control's CHECK pins it to viewer or editor.
const roomJoinRole = z.enum(['viewer', 'editor'])

const spaceJoinRoomInput = z.object({
  spaceRef: z.string().min(1),
})

const spaceJoinRoomOutput = z.object({
  space_ref: z.string(),
  role: z.string().nullable(),
  changed: z.boolean(),
  already_member: z.boolean(),
  // Always false at join time: the SPA polls the membership read until the audience is ready.
  recipient_ready: z.boolean(),
  audience: z.object({ state: z.string() }).optional(),
})

const spaceUpdateRoomSettingsInput = z
  .object({
    spaceRef: z.string().min(1),
    visibility: roomVisibility.optional(),
    joinRole: roomJoinRole.optional(),
  })
  .refine((value) => value.visibility !== undefined || value.joinRole !== undefined, {
    message: 'Provide a visibility or a join role.',
  })

const spaceUpdateRoomSettingsOutput = z.object({
  space_ref: z.string(),
  visibility: roomVisibility.nullable(),
  join_role: roomJoinRole.nullable(),
  changed: z.boolean(),
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
  {
    id: 'spaces.join_room',
    label: 'Join Space room',
    description:
      'Join an open room from the organization directory. Only an organization member may; a private room, a personal Space and a room of another organization answer exactly as an unknown room. The role received is the one the room set for self-join (viewer or editor), never manager or owner.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceJoinRoomInput,
    outputSchema: spaceJoinRoomOutput,
  },
  {
    id: 'spaces.update_room_settings',
    label: 'Update Space room visibility',
    description:
      'Set whether a named room is private or open to the organization directory, and the role a self-join receives (viewer or editor). Only a room owner or manager may; opening a room needs the directory policy of the organization to be on, and the organization room and archived rooms are refused.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceUpdateRoomSettingsInput,
    outputSchema: spaceUpdateRoomSettingsOutput,
  },
] as const satisfies readonly ActionDescriptor[]
