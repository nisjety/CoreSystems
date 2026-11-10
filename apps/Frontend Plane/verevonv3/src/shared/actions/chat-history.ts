import { z } from 'zod'

// The client action contract for chat history (WS17). Each action names one
// change to a conversation the server owns, so the browser parses the request
// here before it sends anything. The archive pair and `thread.search` are wired;
// `thread.export` is declared for the export surface (EP-12 S11); the other actions
// are declared so the later history slices extend this union instead of inventing
// parallel names. `thread.search`, `thread.export` and `thread.side_chat` only read.
// `thread.spin_off` creates nothing on the server: the browser opens a draft, and nothing
// is sent until the person sends it. Every other action here changes server state, so
// none of them is a local presentation toggle.
const threadId = z.string().trim().min(1).max(200)
const messageId = z.string().trim().min(1).max(200)

// The owner's copy of one conversation (EP-12 S11). Neither choice has a default: the person
// picks the format, and `redacted` leaves out tool payloads, connector data and attachment names.
const threadExportFields = {
  threadId,
  format: z.enum(['json', 'markdown']),
  profile: z.enum(['full', 'redacted']),
}

// The most an import takes (EP-12 S15): the export file's text, at most 2 MiB as the gateway and Session Core read it.
export const THREAD_IMPORT_MAX_BYTES = 2 * 1024 * 1024

// The owner's import of one export file (EP-12 S15): the file's text, and a title only when one is given; the file's
// own title is used otherwise. The new thread is the caller's, so no owner or organisation is named here.
const threadImportFields = {
  exportJson: z
    .string()
    .min(1)
    .refine((text) => new TextEncoder().encode(text).length <= THREAD_IMPORT_MAX_BYTES, {
      message: 'The file is larger than an import takes.',
    }),
  title: z.string().trim().max(96).optional(),
}

// The three members the command palette runs (W5-077-c) are named, so the palette's inputs reuse them
// instead of restating the fields. The union below is unchanged.
export const threadArchiveAction = z.object({ type: z.literal('thread.archive'), threadId })
export const threadStarAction = z.object({ type: z.literal('thread.star'), threadId, starred: z.boolean() })
export const threadMarkUnreadAction = z.object({ type: z.literal('thread.mark_unread'), threadId })

// A side question (F-t3-069) only reads: Model Gateway answers from a snapshot of the thread and keeps nothing.
export const threadSideChatAction = z.object({
  type: z.literal('thread.side_chat'),
  threadId,
  question: z.string().trim().min(1).max(2000),
})

// A spin-off (F-t3-069) creates nothing server-side. The browser opens a new draft chat with this text in its
// composer, and the chat exists on the server only once the person sends it.
export const threadSpinOffAction = z.object({
  type: z.literal('thread.spin_off'),
  threadId,
  draft: z.string().trim().min(1).max(4000),
})

export const chatHistoryActionSchema = z.discriminatedUnion('type', [
  threadArchiveAction,
  z.object({ type: z.literal('thread.unarchive'), threadId }),
  z.object({ type: z.literal('thread.fork'), threadId, fromMessageId: messageId, title: z.string().trim().max(96).optional() }),
  threadStarAction,
  threadMarkUnreadAction,
  z.object({ type: z.literal('thread.mark_read'), threadId }),
  z.object({ type: z.literal('thread.search'), query: z.string().trim().min(1).max(200), threadId: threadId.optional() }),
  z.object({ type: z.literal('thread.export'), ...threadExportFields }),
  z.object({ type: z.literal('thread.import'), ...threadImportFields }),
  z.object({ type: z.literal('message.switch_version'), threadId, messageId, index: z.number().int().min(0).max(100) }),
  // A rewind moves the active leaf and deletes nothing, but it still changes
  // which answer the next turn sees, so the caller must confirm it explicitly.
  z.object({ type: z.literal('thread.rewind'), threadId, messageId, confirm: z.literal(true) }),
  // An undo returns the leaf to the turn a rewind left. It moves nothing the rewind did not already move, so it
  // needs no confirmation of its own.
  z.object({ type: z.literal('thread.undo_rewind'), threadId, messageId }),
  threadSideChatAction,
  threadSpinOffAction,
])

// The registry's input for the same export: the three fields and nothing else. The owner is the
// session's, so a body that names an owner or an organisation is refused rather than dropped.
export const threadExportInput = z.object(threadExportFields).strict()

// The registry's input for the import: the file's text and the optional title, and nothing else. The new thread is the
// session's, so a body that names an owner or an organisation is refused rather than dropped.
export const threadImportInput = z.object(threadImportFields).strict()

export type ChatHistoryAction = z.infer<typeof chatHistoryActionSchema>

export function parseChatHistoryAction(input: unknown): ChatHistoryAction {
  return chatHistoryActionSchema.parse(input)
}
