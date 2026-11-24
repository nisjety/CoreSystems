import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS17-S5: personal agents in product. Four human-only contracts. The gateway routes are
// apps/gateway/src/domains/spaces/personal_agents.rs (dispatchers in dispatchers_spaces/ws17.rs).
// Control owns the agent registry, the access list and the room admission; Application
// (convex-core) owns the agent definition and the primary-agent preference. None of the four is
// model-executable (model-access.ts closes them): an agent must not create agents, widen who may
// add them, choose its owner's primary agent or withdraw itself. They are allowed under Zero Data
// Retention (WS17 design D7): configuration that carries ids only, no conversation content.
// `spaces.withdraw_agent` is the owner's or manager's room-side removal; `spaces.revoke_agent`
// (base.ts) stays a separate control.

const agentsCreatePersonalInput = z.object({
  name: z.string().trim().min(2).max(60),
  instructions: z.string().trim().max(4000).optional(),
  avatarColor: z
    .string()
    .trim()
    .regex(/^#[0-9a-fA-F]{6}$/, 'avatarColor must be a 6-digit hex color')
    .optional(),
  clientRequestId: z.string().trim().min(1).max(128),
})

const agentsCreatePersonalOutput = z.object({
  agent_ref: z.string(),
  subject_id: z.string(),
  visibility: z.literal('owner'),
  add_policy: z.literal('owner_only'),
  revision: z.number(),
})

const agentAccessEntryRole = z.enum(['manager', 'collaborator'])

const agentsUpdateAccessInput = z.object({
  agentRef: z.string().min(1),
  visibility: z.enum(['owner', 'org']),
  addPolicy: z.enum(['owner_only', 'nobody', 'anyone']),
  entries: z
    .array(z.object({ principalId: z.string().min(1), role: agentAccessEntryRole }))
    .max(200),
  // The revision the caller read. Control refuses a stale save (409 agent_state_changed).
  expectedRevision: z.number().int().min(0),
})

const agentsUpdateAccessOutput = z.object({
  agent_ref: z.string(),
  subject_id: z.string(),
  role: z.enum(['owner', 'manager', 'collaborator']),
  visibility: z.enum(['owner', 'org']),
  add_policy: z.enum(['owner_only', 'nobody', 'anyone']),
  revision: z.number(),
  entries: z.array(z.object({ principal_id: z.string(), role: agentAccessEntryRole })),
})

const agentsSetPrimaryInput = z.object({
  agentRef: z.string().min(1).nullable(),
})

const agentsSetPrimaryOutput = z.object({
  primary_subject_id: z.string().nullable(),
})

const spacesWithdrawAgentInput = z.object({
  spaceRef: z.string().min(1),
  agentRef: z.string().min(1),
})

const spacesWithdrawAgentOutput = z.object({
  agent_ref: z.string(),
  space_ref: z.string(),
  state: z.literal('withdrawn'),
  changed: z.boolean(),
})

// WS17-S6: the owner's private panel. One human-only contract: the gateway get-or-creates the caller's
// panel pointer for one room (`GET`/`POST /api/v1/spaces/{ref}/private-panel`). The panel is an owner
// thread with `sourceScope:'conversation'` (SD-22 v1: no memory in or out). Sharing a reply to the room
// is a human post through `spaces.create_post` (ws01.ts), not an action of this contract. A turn typed in
// the panel's own composer (D21e) is an ordinary chat turn on that owner thread (`POST /api/v1/chat/stream`,
// conversation scope, no `space_ref`), like a turn typed in Chat: no action descriptor, and no room write.
const spacesOpenPrivatePanelInput = z.object({
  spaceRef: z.string().min(1),
})

const spacesOpenPrivatePanelOutput = z.object({
  space_ref: z.string(),
  thread_id: z.string().nullable(),
  agent_subject_id: z.string().nullable(),
})

// WS17-S7: a person shares one of their own personal memories into a room, after reading its exact text
// (`POST /api/v1/spaces/{ref}/memory/declassify`, 201 `{data:{room_memory_id, space_ref, source_memory_id, deduplicated}}`).
// The browser sends the memory id, the reviewed text and a retry key, and nothing else: the gateway and Control mint the
// subject, the audience and the content digest, and refuse any other field. The room copy is a Session Core row, so the
// owner plane is `model`. Human only: an agent never shares a person's memory.
const memoryDeclassifyToSpaceInput = z.object({
  memoryId: z.string().min(1),
  spaceRef: z.string().min(1),
  // Code points, as the server counts them: at most 1000 characters (session-core `DeclassifyMemory`).
  reviewedContent: z
    .string()
    .trim()
    .min(1)
    .refine((text) => [...text].length <= 1000, 'reviewedContent must be at most 1000 characters'),
  clientRequestId: z.string().trim().min(1).max(128),
})

const memoryDeclassifyToSpaceOutput = z.object({
  room_memory_id: z.string(),
  space_ref: z.string(),
  source_memory_id: z.string(),
  deduplicated: z.boolean(),
})

// WS17-S7b: a person takes back a room copy they shared (SD-22: the declassifying person retracts; a room manager
// cannot). `POST /api/v1/spaces/{ref}/memory/retract`, 200 `{data:{memory_id, space_ref, deleted}}`. The browser sends
// the room copy's id and a retry key and nothing else. Allowed under Zero Data Retention: a removal stores nothing.
// Human only: an agent never takes back a person's share.
const memoryRetractFromSpaceInput = z.object({
  memoryId: z.string().min(1),
  spaceRef: z.string().min(1),
  clientRequestId: z.string().trim().min(1).max(128),
})

const memoryRetractFromSpaceOutput = z.object({
  memory_id: z.string(),
  space_ref: z.string(),
  deleted: z.boolean(),
})

export const ws17SpaceActions = [
  {
    id: 'agents.create_personal',
    label: 'Create personal agent',
    description:
      'Create an agent that only you own. It is not in any room until you add it to one. You choose who may see it and who may add it.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: agentsCreatePersonalInput,
    outputSchema: agentsCreatePersonalOutput,
  },
  {
    id: 'agents.update_access',
    label: 'Update agent access',
    description:
      'Change who can see and add one of your agents, and who holds manager or collaborator access. Saved only against the revision you read; a save based on an older revision is refused.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentsUpdateAccessInput,
    outputSchema: agentsUpdateAccessOutput,
  },
  {
    id: 'agents.set_primary',
    label: 'Set primary agent',
    description:
      'Choose which of your agents is your primary one, or clear it. This is a preference only, and it is checked again each time it is used.',
    ownerPlane: 'application',
    risk: 'medium',
    requiresApproval: false,
    reversible: true,
    inputSchema: agentsSetPrimaryInput,
    outputSchema: agentsSetPrimaryOutput,
  },
  {
    id: 'spaces.withdraw_agent',
    label: 'Withdraw agent from room',
    description:
      'Take an agent out of one room. The owner or a room manager can do this. The agent itself is kept, and it can be added to the room again later.',
    ownerPlane: 'control',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spacesWithdrawAgentInput,
    outputSchema: spacesWithdrawAgentOutput,
  },
  {
    id: 'spaces.open_private_panel',
    label: 'Open private panel',
    description:
      'Open your private assistant for this room. Only you can see it. Nothing in it is posted to the room or remembered there.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spacesOpenPrivatePanelInput,
    outputSchema: spacesOpenPrivatePanelOutput,
  },
  {
    id: 'memory.declassify_to_space',
    label: 'Share memory to room',
    description:
      'Share one of your own personal memories with a room, as you, exactly as you reviewed it. Everyone in the room can see it, and the agents in the room can use it. Your own memory is not changed. Refused under Zero Data Retention.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: memoryDeclassifyToSpaceInput,
    outputSchema: memoryDeclassifyToSpaceOutput,
  },
  {
    id: 'memory.retract_from_space',
    label: 'Take memory back from room',
    description:
      'Take back a memory you shared with a room. It is removed from the room for everyone. Your own memory is not changed. Allowed under Zero Data Retention.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: memoryRetractFromSpaceInput,
    outputSchema: memoryRetractFromSpaceOutput,
  },
] as const satisfies readonly ActionDescriptor[]
