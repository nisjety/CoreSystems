import { z } from 'zod'
import type { ActionDescriptor } from '@/shared/actions/types'

// WS11-S3: "Mark all read", "Mark unread" and the per-room notification level; WS11-S7: the
// attention inbox and the thread follow control. All write only the caller's own
// Application rows (ids, a closed word and a time; never text). The gateway is
// apps/gateway/src/domains/spaces/attention.rs, the dispatcher dispatchers_spaces/ws11.rs.
// No id here is model-executable (model-access.ts closes them): an inbox, a read position, a level
// and a follow are a person's own attention. `spaces.set_thread_follow` is refused with the typed
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

const spaceMarkAllReadInput = z.object({
  // At most 60 rooms; absent means every room the caller belongs to.
  spaceRefs: z.array(z.string().trim().min(1).max(200)).max(60).optional(),
  // Only read-driven items are resolved; "needs you" is live and never touched.
  kinds: z.array(z.enum(['mention', 'reply'])).optional(),
})

const spaceMarkAllReadOutput = z.object({
  rooms_marked: z.number(),
  items_resolved: z.number(),
  marked_at: z.number(),
})

const spaceMarkUnreadInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  // Epoch milliseconds: everything newer reads as unread again. Never moves the position forward.
  beforeMs: z.number().int().positive(),
})

const spaceMarkUnreadOutput = z.object({
  last_read_at: z.number().nullable(),
  manual_unread: z.boolean(),
})

const spaceSetNotifyLevelInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  level: z.enum(['all', 'mentions', 'off']).optional(),
  hidePreview: z.boolean().optional(),
})

const spaceSetNotifyLevelOutput = z.object({
  level: z.enum(['all', 'mentions', 'off']),
  hide_preview: z.boolean(),
})

const spaceSetStatusInput = z.object({
  state: z.enum(['available', 'away', 'dnd', 'offline']),
  // At most 80 characters; refused (zdr_durable_mutation_forbidden) for a Zero Data Retention organization.
  text: z.string().max(200).optional(),
  // Epoch milliseconds, in the future and at most 7 days ahead. Absent means until changed.
  expiresAt: z.number().int().positive().optional(),
})

const spaceSetStatusOutput = z.object({
  state: z.enum(['available', 'away', 'dnd', 'offline']),
  text: z.string().nullable(),
  expires_at: z.number().nullable(),
})

const spaceCreateReminderInput = z.object({
  spaceRef: z.string().trim().min(1).max(200),
  threadId: z.string().trim().min(1).max(200),
  messageId: z.string().trim().min(1).max(200).optional(),
  // Epoch milliseconds, at least five seconds and at most 366 days ahead.
  remindAt: z.number().int().positive(),
})

const spaceCreateReminderOutput = z.object({
  reminder_id: z.string(),
  remind_at: z.number(),
  deduplicated: z.boolean(),
})

const spaceCancelReminderInput = z.object({
  reminderId: z.string().trim().min(1).max(200),
})

const spaceCancelReminderOutput = z.object({
  reminder_id: z.string(),
  cancelled: z.boolean(),
})

export const ws11SpaceActions = [
  {
    id: 'spaces.mark_all_read',
    label: 'Mark all rooms read',
    description:
      'Move your own read position in every room (or the rooms named) up to now and clear your read-driven mention and reply items. Nothing is created and nobody else is affected; approvals waiting on you stay.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceMarkAllReadInput,
    outputSchema: spaceMarkAllReadOutput,
  },
  {
    id: 'spaces.mark_unread',
    label: 'Mark a room unread',
    description:
      'Move your own read position in one room back to just before a time, so newer posts show as unread again. Only your position changes.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceMarkUnreadInput,
    outputSchema: spaceMarkUnreadOutput,
  },
  {
    id: 'spaces.set_notify_level',
    label: 'Set room notifications',
    description:
      'Choose how much of one room reaches your attention: everything, only mentions, or nothing, and whether previews are hidden. It only changes what you are notified about.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetNotifyLevelInput,
    outputSchema: spaceSetNotifyLevelOutput,
  },
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
  {
    id: 'spaces.set_status',
    label: 'Set your status',
    description:
      'Set your own status in this organization: available, away, do not disturb or offline, with an optional short text and expiry (up to 7 days). Only you set it; do-not-disturb pauses out-of-app notifications only.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceSetStatusInput,
    outputSchema: spaceSetStatusOutput,
  },
  {
    id: 'spaces.create_reminder',
    label: 'Remind me about a post',
    description:
      'Set a reminder for yourself on one post (or one message) of a room you belong to. At the time you choose it appears in your attention inbox as an id-only item; there is no note. Refused for a Zero Data Retention organization. At most 50 pending reminders.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: true,
    inputSchema: spaceCreateReminderInput,
    outputSchema: spaceCreateReminderOutput,
  },
  {
    id: 'spaces.cancel_reminder',
    label: 'Cancel a reminder',
    description: 'Cancel one of your own pending reminders. Nobody else is affected.',
    ownerPlane: 'application',
    risk: 'low',
    requiresApproval: false,
    reversible: false,
    inputSchema: spaceCancelReminderInput,
    outputSchema: spaceCancelReminderOutput,
  },
] as const satisfies readonly ActionDescriptor[]
