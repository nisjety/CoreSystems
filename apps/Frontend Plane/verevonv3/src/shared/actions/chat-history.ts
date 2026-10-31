import { z } from 'zod'

// The client action contract for chat history (WS17). Each action names one
// change to a conversation the server owns, so the browser parses the request
// here before it sends anything. The archive pair and `thread.search` are wired;
// the other actions are declared so the later history slices extend this union
// instead of inventing parallel names. `thread.search` only reads; every other
// action here changes server state, so none of them is a local presentation toggle.
const threadId = z.string().trim().min(1).max(200)
const messageId = z.string().trim().min(1).max(200)

export const chatHistoryActionSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('thread.archive'), threadId }),
  z.object({ type: z.literal('thread.unarchive'), threadId }),
  z.object({ type: z.literal('thread.fork'), threadId, fromMessageId: messageId, title: z.string().trim().max(96).optional() }),
  z.object({ type: z.literal('thread.star'), threadId, starred: z.boolean() }),
  z.object({ type: z.literal('thread.mark_unread'), threadId }),
  z.object({ type: z.literal('thread.mark_read'), threadId }),
  z.object({ type: z.literal('thread.search'), query: z.string().trim().min(1).max(200), threadId: threadId.optional() }),
  z.object({ type: z.literal('message.switch_version'), threadId, messageId, index: z.number().int().min(0).max(100) }),
  // A rewind moves the active leaf and deletes nothing, but it still changes
  // which answer the next turn sees, so the caller must confirm it explicitly.
  z.object({ type: z.literal('thread.rewind'), threadId, messageId, confirm: z.literal(true) }),
])

export type ChatHistoryAction = z.infer<typeof chatHistoryActionSchema>

export function parseChatHistoryAction(input: unknown): ChatHistoryAction {
  return chatHistoryActionSchema.parse(input)
}
