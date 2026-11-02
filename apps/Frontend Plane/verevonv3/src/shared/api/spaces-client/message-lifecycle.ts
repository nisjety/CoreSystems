// WS12-S7: edit and delete one message of a Space thread.
//
//   PATCH  /api/v1/spaces/{ref}/threads/{thread_id}/messages/{message_id}  {content, idempotency_key}
//   DELETE /api/v1/spaces/{ref}/threads/{thread_id}/messages/{message_id}  header Idempotency-Key,
//                                                                          optional ?moderation_reason=
//     -> 200 { data: { message_id, revision_state: 'edited'|'deleted', revision_no, revised_at, deduplicated } }
// Errors are the typed `{error:{code,message,details}}` envelope and arrive as an ApiError
// (`space_message_revise_disabled`, `space_message_revise_not_authorized`, `message_revise_forbidden`,
// `message_not_found`, `message_already_deleted`, `thread_run_active`, `revision_noop`,
// `idempotency_key_conflict`, `space_authority_stale`, the typed `zdr_*` refusals, ...). Copy for each
// code lives in shared/i18n/errors.ts under the WS12-S7 banner.
//
// `idempotencyKey` names ONE attempt for its whole life: a retry passes the same key and the same
// text and lands on the same revision (`deduplicated: true`).
import { requestJson } from '../http'

/** The closed set a moderator picks from when deleting someone else's message. */
export const SPACE_MODERATION_REASONS = ['policy_violation', 'spam', 'harassment', 'privacy', 'other'] as const
export type SpaceModerationReason = (typeof SPACE_MODERATION_REASONS)[number]

export type SpaceMessageRevisionReceipt = {
  readonly messageId: string
  readonly revisionState: 'edited' | 'deleted'
  readonly revisionNo: number
  readonly revisedAt?: string
  readonly deduplicated: boolean
}

type ReceiptWire = {
  message_id?: unknown
  revision_state?: unknown
  revision_no?: unknown
  revised_at?: unknown
  deduplicated?: unknown
}

/** A fresh key for one edit or delete attempt (32 hex characters, inside the gateway's 16..64 shape). */
export function newMessageRevisionKey(): string {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function receiptFrom(wire: ReceiptWire | null | undefined): SpaceMessageRevisionReceipt {
  const messageId = typeof wire?.message_id === 'string' ? wire.message_id : ''
  const state = wire?.revision_state
  // An answer this browser cannot read is not a success it can show: fail loudly so the change
  // stays retryable (the retry is deduplicated by its key).
  if (!messageId || (state !== 'edited' && state !== 'deleted')) throw new Error('message_revision_unreadable')
  return {
    messageId,
    revisionState: state,
    revisionNo: typeof wire?.revision_no === 'number' && Number.isFinite(wire.revision_no) ? wire.revision_no : 0,
    ...(typeof wire?.revised_at === 'string' && wire.revised_at ? { revisedAt: wire.revised_at } : {}),
    deduplicated: wire?.deduplicated === true,
  }
}

function messagePath(spaceRef: string, threadId: string, messageId: string): string {
  return `/api/v1/spaces/${encodeURIComponent(spaceRef)}/threads/${encodeURIComponent(threadId)}/messages/${encodeURIComponent(messageId)}`
}

/** Replace the text of one message. */
export async function editSpaceMessage(
  spaceRef: string,
  threadId: string,
  messageId: string,
  input: { readonly content: string; readonly idempotencyKey: string },
): Promise<SpaceMessageRevisionReceipt> {
  const wire = await requestJson<ReceiptWire>(messagePath(spaceRef, threadId, messageId), {
    method: 'PATCH',
    body: JSON.stringify({ content: input.content, idempotency_key: input.idempotencyKey }),
  })
  return receiptFrom(wire)
}

/** Delete one message; every reader then sees a tombstone in its place. */
export async function deleteSpaceMessage(
  spaceRef: string,
  threadId: string,
  messageId: string,
  input: { readonly idempotencyKey: string; readonly moderationReason?: SpaceModerationReason },
): Promise<SpaceMessageRevisionReceipt> {
  const query = input.moderationReason ? `?moderation_reason=${encodeURIComponent(input.moderationReason)}` : ''
  const wire = await requestJson<ReceiptWire>(`${messagePath(spaceRef, threadId, messageId)}${query}`, {
    method: 'DELETE',
    headers: { 'Idempotency-Key': input.idempotencyKey },
  })
  return receiptFrom(wire)
}
