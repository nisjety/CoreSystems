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

const postReceiptOutput = z.object({
  thread_id: z.string(),
  message_id: z.string(),
  sequence: z.number().int(),
  deduplicated: z.boolean(),
})

const spaceCreatePostInput = z.object({
  spaceRef: z.string().min(1),
  content: postContent,
  clientSendId,
})

const spaceReplyInThreadInput = z.object({
  spaceRef: z.string().min(1),
  threadId: z.string().min(1),
  content: postContent,
  clientSendId,
})

export const ws01SpaceActions = [
  {
    id: 'spaces.create_post',
    label: 'Post in Space room',
    description:
      'Start a new thread in a named room with a message from you. It is stored as your own post and starts no agent run. Control decides whether you may post; a retry with the same clientSendId lands as the same post.',
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
      'Reply in an existing room thread with a message from you. It is stored as your own post and starts no agent run. Control decides whether you may post; a retry with the same clientSendId lands as the same post.',
    ownerPlane: 'model',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceReplyInThreadInput,
    outputSchema: postReceiptOutput,
  },
] as const satisfies readonly ActionDescriptor[]
