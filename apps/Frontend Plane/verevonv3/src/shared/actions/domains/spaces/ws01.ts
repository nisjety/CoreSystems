import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS01-S5: human post contracts. A post is a person speaking in a room: stored by Session Core
// with no model run, under a Control-signed `thread:append` decision the gateway mints per send
// (apps/gateway/src/domains/spaces/human_posts.rs, dispatchers_spaces/ws01.rs). Neither id is
// model-executable: a post is never an agent proposal (model-access.ts closes both,
// model-eligibility.ts never lists them). A Zero Data Retention organization is refused with the
// typed `zdr_durable_mutation_forbidden` before any decision is minted.

// Same bounds as the gateway: 1..8000 characters (trimmed server-side) and a send id the retry
// reuses so one send lands as one row.
const postContent = z.string().trim().min(1).max(8000)
const clientSendId = z.string().regex(/^[A-Za-z0-9_-]{16,64}$/)

// DEPTH-WS08-S6-3 (C-PRIO-03): the post's structured mentions, the same ids the composer sends on
// the REST routes (shared/api/spaces-client/mentions.ts). A mention is a Control subject id, never
// text the server parses; the gateway checks each one against the room's roster and bindings with
// the same planning code as the REST routes, so this schema authorizes nothing. Caps match the
// gateway (20 people, 3 agents, a label of at most 120 characters). A mention never fails the
// post: a ref the server cannot honour comes back under `mentions.rejected`. A Zero Data Retention
// organization is still refused before any mention is planned.
const mentionSubjectId = z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/)
const mentionSource = z.enum(['typed', 'locked'])

const postMentionsInput = z
  .object({
    people: z
      .array(z.object({ subjectId: mentionSubjectId, label: z.string().trim().max(120), source: mentionSource.optional() }).strict())
      .max(20)
      .optional(),
    agents: z
      .array(z.object({ subjectId: mentionSubjectId, source: mentionSource.optional() }).strict())
      .max(3)
      .optional(),
  })
  .strict()

const postMentionsReceipt = z.object({
  recorded: z.array(
    z.object({
      subject_id: z.string(),
      kind: z.enum(['user', 'agent']),
      wake: z.enum(['expected', 'refused']).optional(),
      reason: z.string().optional(),
      ordinal: z.number().int().optional(),
    }),
  ),
  rejected: z.array(z.object({ subject_id: z.string(), code: z.string() })),
})

const postReceiptOutput = z.object({
  thread_id: z.string(),
  message_id: z.string(),
  sequence: z.number().int(),
  deduplicated: z.boolean(),
  // Present only when the post carried mentions and the server planned them.
  mentions: postMentionsReceipt.optional(),
})

const spaceCreatePostInput = z.object({
  spaceRef: z.string().min(1),
  content: postContent,
  clientSendId,
  mentions: postMentionsInput.optional(),
})

const spaceReplyInThreadInput = z.object({
  spaceRef: z.string().min(1),
  threadId: z.string().min(1),
  content: postContent,
  clientSendId,
  mentions: postMentionsInput.optional(),
})

export const ws01SpaceActions = [
  {
    id: 'spaces.create_post',
    label: 'Post in Space room',
    description:
      'Start a new thread in a named room with a message from you. It is stored as your own post and starts no agent run. Control decides whether you may post; a retry with the same clientSendId lands as the same post. Optional mentions name people and agents by subject id; the server checks each against the room and reports any it could not record.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceCreatePostInput,
    outputSchema: postReceiptOutput.extend({ space_ref: z.string() }),
  },
  {
    id: 'spaces.reply_in_thread',
    label: 'Reply in Space thread',
    description:
      'Reply in an existing room thread with a message from you. It is stored as your own post and starts no agent run. Control decides whether you may post; a retry with the same clientSendId lands as the same post. Optional mentions name people and agents by subject id; the server checks each against the room and reports any it could not record.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceReplyInThreadInput,
    outputSchema: postReceiptOutput,
  },
] as const satisfies readonly ActionDescriptor[]
