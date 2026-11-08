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

// WS12-S7: message lifecycle. `spaces.edit_message` replaces the text of one message and
// `spaces.delete_message` leaves a tombstone for every reader. Control mints a
// `model.thread.revise` decision bound to the new text, Session Core decides whether the caller is
// the author, the agent owner or a moderator and records the revision (apps/gateway/src/domains/
// spaces/message_lifecycle.rs). Neither id is model-executable (model-access.ts closes both): a
// person changes their own words. A Zero Data Retention organization is refused with the typed
// `zdr_durable_mutation_forbidden` before any Control or model call.
const spaceMessageRevisionOutput = z.object({
  message_id: z.string(),
  revision_state: z.enum(['edited', 'deleted']),
  revision_no: z.number(),
  revised_at: z.string().nullable(),
  deduplicated: z.boolean(),
})

const spaceEditMessageInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  threadId: z.string().trim().min(1).max(128),
  messageId: z.string().trim().min(1).max(128),
  content: z.string().trim().min(1).max(8000),
  // One key per attempt; a retry with the same key and text lands on the same revision.
  idempotencyKey: z.string().regex(/^[A-Za-z0-9_-]{16,64}$/),
})

const spaceDeleteMessageInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  threadId: z.string().trim().min(1).max(128),
  messageId: z.string().trim().min(1).max(128),
  // Honoured only when a room manager deletes someone else's message.
  moderationReason: z.enum(['policy_violation', 'spam', 'harassment', 'privacy', 'other']).optional(),
  idempotencyKey: z.string().regex(/^[A-Za-z0-9_-]{16,64}$/),
})

// WS12-S8: reactions. `spaces.toggle_reaction` adds (or removes) the caller's one emoji on one
// message. Application stores ids, the emoji and a time (apps/gateway/src/domains/spaces/
// message_lifecycle.rs); the live room role (editor or above), Control's shared-read decision and
// Session Core's message check decide. Not model-executable: a reaction is a person's own mark and
// never an approval. A Zero Data Retention organization is refused the typed
// `zdr_durable_mutation_forbidden` before any write.
const spaceToggleReactionInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  threadId: z.string().trim().min(1).max(128),
  messageId: z.string().trim().min(1).max(128),
  // One emoji cluster; Application checks it exactly.
  emoji: z.string().min(1).max(32),
  // Absent adds; false removes the caller's reaction.
  present: z.boolean().optional(),
})

const spaceToggleReactionOutput = z.object({
  message_id: z.string(),
  reactions: z.array(z.object({ emoji: z.string(), count: z.number(), reacted: z.boolean() })),
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
  {
    id: 'spaces.edit_message',
    label: 'Edit message',
    description:
      'Replace the text of one of your own messages in a Space thread. Everyone sees it marked as edited; the model sees the edited marker on its next turn. Control must allow message changes for the organization.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceEditMessageInput,
    outputSchema: spaceMessageRevisionOutput,
  },
  {
    id: 'spaces.delete_message',
    label: 'Delete message',
    description:
      'Delete one message in a Space thread. Everyone sees a tombstone in its place and the text is no longer available; replies and actions that followed it are not undone. You can delete your own messages, an agent answer to your run, and, as a room manager or owner where the organization allows it, other members messages.',
    ownerPlane: 'model',
    risk: 'medium',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceDeleteMessageInput,
    outputSchema: spaceMessageRevisionOutput,
  },
  {
    id: 'spaces.toggle_reaction',
    label: 'React to message',
    description:
      'Add your emoji reaction to one message in a Space thread, or remove it. A reaction is a mark for the room: it does not start an agent run, is not an approval and is not read as an instruction. Viewers cannot react.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceToggleReactionInput,
    outputSchema: spaceToggleReactionOutput,
  },
] as const satisfies readonly ActionDescriptor[]
