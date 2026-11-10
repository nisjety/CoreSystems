import { z } from 'zod'

// The client action contract for the chat library (F-t3-080): the person's saved chat files, generated
// artifact copies and uploaded text, read as one filtered view of their own Data Plane documents. The
// browser parses each operation here before it sends anything, and the registry (action-registry.ts)
// takes the same input shapes.
//
// `library.list`, `library.read_text` and `library.open_origin` only read or navigate. `library.share`
// writes resource grants, so it is grant-only: a grant names one org member or group and a role. No
// member makes a public link, and a key the contract does not name (a link, a visibility) is refused
// rather than dropped, so a request can never widen what it grants.

const documentId = z.string().trim().min(1).max(200)
const threadId = z.string().trim().min(1).max(200)
const principalId = z.string().trim().min(1).max(200)

// The gateway serves one to 100 rows a page and refuses 0, so the contract refuses 0 as well.
const libraryListFields = {
  kind: z.enum(['generated', 'uploaded', 'all']),
  limit: z.number().int().min(1).max(100).default(20),
  offset: z.number().int().min(0).default(0),
}

const libraryGrant = z
  .object({
    principalType: z.enum(['user', 'group']),
    principalId,
    role: z.enum(['viewer', 'editor']),
  })
  .strict()

const libraryShareFields = {
  documentId,
  grants: z.array(libraryGrant).min(1).max(50),
}

/** The document id a library read or share names. The share's grants are checked by its own input. */
export const chatLibraryDocumentIdSchema = documentId

export const chatLibraryListInput = z.object(libraryListFields).strict()
export const chatLibraryReadTextInput = z.object({ documentId }).strict()
export const chatLibraryOpenOriginInput = z.object({ threadId }).strict()
export const chatLibraryShareInput = z.object(libraryShareFields).strict()

export const chatLibraryActionSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('library.list'), ...libraryListFields }).strict(),
  z.object({ type: z.literal('library.read_text'), documentId }).strict(),
  z.object({ type: z.literal('library.open_origin'), threadId }).strict(),
  z.object({ type: z.literal('library.share'), ...libraryShareFields }).strict(),
])

export type ChatLibraryAction = z.infer<typeof chatLibraryActionSchema>
export type ChatLibraryActionType = ChatLibraryAction['type']

/** One operation's member of the contract, narrowed to its type. */
export type ChatLibraryOperation<T extends ChatLibraryActionType> = Extract<ChatLibraryAction, { type: T }>

/**
 * A library operation the contract refuses. Nothing is sent when it is thrown. `action` names the
 * operation and never carries the input, so a refusal can be logged without the person's data.
 */
export class ChatLibraryContractError extends Error {
  readonly code = 'chat_library_contract_refused'
  readonly action: string

  constructor(action: string) {
    super(`The chat library ${action} request is refused by its action contract.`)
    this.name = 'ChatLibraryContractError'
    this.action = action
  }
}

/**
 * Parses one operation through the contract and returns its narrowed member, or throws a typed refusal.
 * The panel calls this before each read, so the request it makes is the parsed one.
 */
export function parseChatLibraryOperation<T extends ChatLibraryActionType>(
  type: T,
  input: Record<string, unknown>,
): ChatLibraryOperation<T> {
  const parsed = chatLibraryActionSchema.safeParse({ ...input, type })
  if (!parsed.success || parsed.data.type !== type) throw new ChatLibraryContractError(type)
  return parsed.data as ChatLibraryOperation<T>
}
