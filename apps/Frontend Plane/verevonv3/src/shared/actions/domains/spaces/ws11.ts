import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS11-S7: the attention inbox and the thread follow control. Both write only the caller's own
// Application rows (ids, a closed word and a time; never text). The gateway is
// apps/gateway/src/domains/spaces/attention.rs, the dispatcher dispatchers_spaces/ws11.rs.
// Neither id is model-executable (model-access.ts closes both): an inbox and a follow are a
// person's own attention. `spaces.set_thread_follow` is refused with the typed
// `zdr_durable_mutation_forbidden` for a Zero Data Retention organization when it sets a follow
// (clearing one is always allowed).

const spaceSetAttentionItemStateInput = z.object({
  itemId: z.string().trim().min(1).max(200),
  state: z.enum(['done', 'open', 'snoozed']),
  // Epoch milliseconds, at least a minute and at most 30 days ahead. Only with state 'snoozed'.
  snoozeUntil: z.number().int().positive().optional(),
})

const spaceSetAttentionItemStateOutput = z.object({
  item_id: z.string(),
  state: z.enum(['done', 'open', 'snoozed']),
  snoozed_until: z.number().nullable(),
})

const spaceSetThreadFollowInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  threadId: z.string().trim().min(1).max(200),
  state: z.enum(['following', 'muted', 'none']),
})

const spaceSetThreadFollowOutput = z.object({
  state: z.enum(['following', 'muted', 'none']),
  reason: z.string().nullable(),
})

export const ws11SpaceActions = [
  {
    id: 'spaces.set_attention_item_state',
    label: 'Mark an inbox item',
    description:
      'Mark one of your own attention inbox items done, open it again, or snooze it until a later time (a minute to 30 days). Nothing else changes and nobody else sees it.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetAttentionItemStateInput,
    outputSchema: spaceSetAttentionItemStateOutput,
  },
  {
    id: 'spaces.set_thread_follow',
    label: 'Follow or mute a post',
    description:
      'Follow one post in a room so its replies reach your attention inbox, mute it, or clear your choice. It only changes what you are notified about.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetThreadFollowInput,
    outputSchema: spaceSetThreadFollowOutput,
  },
] as const satisfies readonly ActionDescriptor[]
