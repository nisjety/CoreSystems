// WS01-S6: human posts. A post is a person speaking in a room: the gateway mints a
// Control-signed `thread:append` decision per send and Session Core stores the message with
// no model run. The browser sends only the text and its retry identity.
//
// Wire (plan WS01, API paragraph):
//   POST /api/v1/spaces/{ref}/threads                     {content, client_send_id}
//     -> 201 (200 when deduplicated) { data: { space_ref, thread_id, message_id, sequence, deduplicated } }
//   POST /api/v1/spaces/{ref}/threads/{thread_id}/messages {content, client_send_id}
//     -> 201 (200 when deduplicated) { data: { thread_id, message_id, sequence, deduplicated } }
// Errors are the typed `{error:{code,message,details}}` envelope and arrive as an ApiError
// (`space_post_not_authorized`, `invalid_post` with `details.max_chars`, `space_authority_stale`,
// `idempotency_key_conflict`, `post_not_stored`, the typed `zdr_*` refusals, ...). The copy for
// each code lives in shared/i18n/errors.ts under the WS01 banner.
//
// `clientSendId` names ONE send for its whole life: a retry passes the same id, so a repeat the
// server already stored comes back `deduplicated: true` and the room holds the post once.
import { requestJson } from '../http'
import {
  mentionsWire,
  parseMentionsReceipt,
  type SpaceMentionsInput,
  type SpaceMentionsReceipt,
} from './mentions'

/** The longest post the gateway accepts, in characters (`data.limits.post_max_chars` says the live value). */
export const SPACE_POST_MAX_CHARS_DEFAULT = 8000

export type SpacePostReceipt = {
  readonly threadId: string
  readonly messageId: string
  readonly sequence: number
  /** True when the server already had this send and returned the original. */
  readonly deduplicated: boolean
  /** WS08: what the server recorded for the post's mentions. Absent when none were sent or it was not told. */
  readonly mentions?: SpaceMentionsReceipt
}

type PostReceiptWire = {
  thread_id?: unknown
  message_id?: unknown
  sequence?: unknown
  deduplicated?: unknown
  mentions?: unknown
}

function receiptFrom(wire: PostReceiptWire | null | undefined, fallbackThreadId?: string): SpacePostReceipt {
  const threadId = typeof wire?.thread_id === 'string' && wire.thread_id.trim() ? wire.thread_id.trim() : fallbackThreadId
  // A receipt with no thread is not an acceptance this browser can show: fail loudly so the
  // send stays retryable (the retry is deduplicated by its key) instead of claiming a post.
  if (!threadId) throw new Error('post_receipt_missing_thread')
  const mentions = parseMentionsReceipt(wire?.mentions)
  return {
    threadId,
    messageId: typeof wire?.message_id === 'string' ? wire.message_id : '',
    sequence: typeof wire?.sequence === 'number' && Number.isFinite(wire.sequence) ? wire.sequence : 0,
    deduplicated: wire?.deduplicated === true,
    ...(mentions ? { mentions } : {}),
  }
}

/** What a post send takes. `mentions` (WS08) is ids the composer picked; the key and the text are unchanged. */
export type SpacePostRequest = {
  readonly content: string
  readonly clientSendId: string
  readonly mentions?: SpaceMentionsInput
}

function postBody(input: SpacePostRequest): string {
  const mentions = mentionsWire(input.mentions)
  return JSON.stringify({
    content: input.content,
    client_send_id: input.clientSendId,
    ...(mentions ? { mentions } : {}),
  })
}

/** Start a new thread in the room with a post from the caller. */
export async function createSpacePost(
  spaceRef: string,
  input: SpacePostRequest,
): Promise<SpacePostReceipt> {
  const wire = await requestJson<PostReceiptWire>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads`, {
    method: 'POST',
    body: postBody(input),
  })
  return receiptFrom(wire)
}

/** Reply in an existing room thread with a post from the caller. */
export async function replyInSpaceThread(
  spaceRef: string,
  threadId: string,
  input: SpacePostRequest,
): Promise<SpacePostReceipt> {
  const wire = await requestJson<PostReceiptWire>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/messages`,
    {
      method: 'POST',
      body: postBody(input),
    },
  )
  return receiptFrom(wire, threadId)
}
