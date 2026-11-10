import { z } from 'zod'

// The client action contract for chat history (WS17). Each action names one
// change to a conversation the server owns, so the browser parses the request
// here before it sends anything. The archive pair and `thread.search` are wired;
// `thread.export` is declared for the export surface (EP-12 S11); the other actions
// are declared so the later history slices extend this union instead of inventing
// parallel names. `thread.search` and `thread.export` only read; every other action
// here changes server state, so none of them is a local presentation toggle.
const threadId = z.string().trim().min(1).max(200)
const messageId = z.string().trim().min(1).max(200)

// The owner's copy of one conversation (EP-12 S11). Neither choice has a default: the person
// picks the format, and `redacted` leaves out tool payloads, connector data and attachment names.
const threadExportFields = {
  threadId,
  format: z.enum(['json', 'markdown']),
  profile: z.enum(['full', 'redacted']),
}

// The three members the command palette runs (W5-077-c) are named, so the palette's inputs reuse them
// instead of restating the fields. The union below is unchanged.
export const threadArchiveAction = z.object({ type: z.literal('thread.archive'), threadId })
export const threadStarAction = z.object({ type: z.literal('thread.star'), threadId, starred: z.boolean() })
export const threadMarkUnreadAction = z.object({ type: z.literal('thread.mark_unread'), threadId })

export const chatHistoryActionSchema = z.discriminatedUnion('type', [
  threadArchiveAction,
  z.object({ type: z.literal('thread.unarchive'), threadId }),
  z.object({ type: z.literal('thread.fork'), threadId, fromMessageId: messageId, title: z.string().trim().max(96).optional() }),
  threadStarAction,
  threadMarkUnreadAction,
  z.object({ type: z.literal('thread.mark_read'), threadId }),
  z.object({ type: z.literal('thread.search'), query: z.string().trim().min(1).max(200), threadId: threadId.optional() }),
  z.object({ type: z.literal('thread.export'), ...threadExportFields }),
  z.object({ type: z.literal('message.switch_version'), threadId, messageId, index: z.number().int().min(0).max(100) }),
  // A rewind moves the active leaf and deletes nothing, but it still changes
  // which answer the next turn sees, so the caller must confirm it explicitly.
  z.object({ type: z.literal('thread.rewind'), threadId, messageId, confirm: z.literal(true) }),
  // An undo returns the leaf to the turn a rewind left. It moves nothing the rewind did not already move, so it
  // needs no confirmation of its own.
  z.object({ type: z.literal('thread.undo_rewind'), threadId, messageId }),
])

// The registry's input for the same export: the three fields and nothing else. The owner is the
// session's, so a body that names an owner or an organisation is refused rather than dropped.
export const threadExportInput = z.object(threadExportFields).strict()

export type ChatHistoryAction = z.infer<typeof chatHistoryActionSchema>

export function parseChatHistoryAction(input: unknown): ChatHistoryAction {
  return chatHistoryActionSchema.parse(input)
}
