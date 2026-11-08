import { z } from 'zod'

// The client action contract for a thread's own files (WS22-S5, DEC-23). A thread
// keeps the files its sandbox wrote across runs, and the user can read that list
// and reset it. `chat.workspace.list` only reads. `chat.workspace.reset` erases
// the thread's files for good, so the caller must confirm it explicitly, the same
// way a rewind is confirmed in chat-history.ts.
//
// The browser parses the request here before it sends anything, and parses the
// response here before it renders anything. The BFF proxies model-gateway's
// snake_case JSON unchanged, so the wire shapes below are the BFF's contract.

const threadId = z
  .string()
  .trim()
  .regex(/^[A-Za-z0-9_-]{1,128}$/, 'threadId must be a plain key segment')

export const chatWorkspaceActionSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('chat.workspace.list'), threadId }),
  z.object({ type: z.literal('chat.workspace.reset'), threadId, confirm: z.literal(true) }),
])

export type ChatWorkspaceAction = z.infer<typeof chatWorkspaceActionSchema>

export function parseChatWorkspaceAction(input: unknown): ChatWorkspaceAction {
  return chatWorkspaceActionSchema.parse(input)
}

const workspaceFileWire = z.object({
  path: z.string().min(1),
  content_hash: z.string().min(1),
  size_bytes: z.number().int().nonnegative(),
})

const workspaceListingWire = z.object({
  thread_id: threadId,
  revision: z.number().int().nonnegative(),
  files: z.array(workspaceFileWire),
})

const workspaceResetWire = z.object({
  thread_id: threadId,
  files_deleted: z.number().int().nonnegative(),
  object_prefix: z.string().min(1),
})

export type ThreadWorkspaceFile = {
  path: string
  contentHash: string
  sizeBytes: number
}

export type ThreadWorkspaceListing = {
  threadId: string
  revision: number
  files: ThreadWorkspaceFile[]
}

export type ThreadWorkspaceReset = {
  threadId: string
  filesDeleted: number
}

export function parseThreadWorkspaceListing(input: unknown): ThreadWorkspaceListing {
  const wire = workspaceListingWire.parse(input)
  return {
    threadId: wire.thread_id,
    revision: wire.revision,
    files: wire.files.map((file) => ({
      path: file.path,
      contentHash: file.content_hash,
      sizeBytes: file.size_bytes,
    })),
  }
}

export function parseThreadWorkspaceReset(input: unknown): ThreadWorkspaceReset {
  const wire = workspaceResetWire.parse(input)
  return { threadId: wire.thread_id, filesDeleted: wire.files_deleted }
}

export function parseThreadWorkspaceId(input: unknown): string {
  return threadId.parse(input)
}

// The BFF route each action calls (apps/gateway/src/domains/chat.rs). The list is a
// GET and the reset is a DELETE on the same path.
export function threadWorkspacePath(id: string): string {
  return `/api/v1/chat/threads/${encodeURIComponent(parseThreadWorkspaceId(id))}/workspace`
}
