// WS04-S2: a room's name and topic. `PATCH /api/v1/spaces/{space_ref}` answers 404
// `feature_disabled` with the gateway flag off, 403 `room_role_forbidden` for anyone but an
// owner or manager, 400 `room_details_invalid` with `details.{field,reason}`, 404
// `space_not_found`, 409 `space_kind_not_editable` / `room_not_active`, 503
// `room_details_unavailable`. They arrive as an ApiError carrying the code, rendered by
// `translateApiError` from the WS04 banner of shared/i18n/errors.ts.
import { requestJson } from '../http'

export type SpaceRoomDetails = {
  spaceRef: string
  name: string
  /** `null` once cleared or never set. */
  topic: string | null
}

/**
 * Rename a room and/or set its topic. Send only the field being changed; an empty `topic`
 * clears it. The server normalizes (NFC, trim, length) and answers with the stored values.
 */
export async function updateSpaceRoomDetails(
  spaceRef: string,
  details: { readonly name?: string; readonly topic?: string },
): Promise<SpaceRoomDetails> {
  const body: Record<string, string> = {}
  if (details.name !== undefined) body.name = details.name
  if (details.topic !== undefined) body.topic = details.topic
  const response = await requestJson<{ space_ref?: string; name?: string; topic?: string | null }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}`,
    { method: 'PATCH', body: JSON.stringify(body) },
  )
  return {
    spaceRef: response.space_ref ?? spaceRef,
    name: response.name ?? '',
    topic: typeof response.topic === 'string' && response.topic !== '' ? response.topic : null,
  }
}

// Archive and unarchive a room. `POST /api/v1/spaces/{space_ref}/archive` and `/unarchive`
// answer 404 `feature_disabled` with the gateway flag off, 403 `room_role_forbidden`, 404
// `room_not_found`, 409 `room_archived` / `room_not_archivable` (the organization room) /
// `room_state_conflict` / `room_personal_space`, 429 `rate_limited` (with
// `details.retry_after_seconds`) and 503 `room_admin_unavailable` (nothing was changed). They
// arrive as an ApiError carrying the code.
export type SpaceRoomArchiveResult = {
  /** The room's state after the call. */
  state: 'archived' | 'active'
  /** False when the room was already in that state (a repeated call is not an error). */
  changed: boolean
  /** When the room was archived, as the server's string; `null` once unarchived. */
  archivedAt: string | null
}

async function postRoomArchive(
  spaceRef: string,
  action: 'archive' | 'unarchive',
  idempotencyKey?: string,
): Promise<SpaceRoomArchiveResult> {
  // An empty body is fine; a key is the only field the route reads.
  const response = await requestJson<{ state?: string; changed?: boolean; archived_at?: string | null }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/${action}`,
    {
      method: 'POST',
      ...(idempotencyKey ? { body: JSON.stringify({ idempotency_key: idempotencyKey }) } : {}),
    },
  )
  const state = response.state === 'archived' || response.state === 'active'
    ? response.state
    : action === 'archive' ? 'archived' : 'active'
  return {
    state,
    changed: response.changed === true,
    archivedAt: typeof response.archived_at === 'string' && response.archived_at !== '' ? response.archived_at : null,
  }
}

/** Archive a room (owner or manager). The room stays readable; writes are refused until it is unarchived. */
export function archiveSpaceRoom(spaceRef: string, options: { readonly idempotencyKey?: string } = {}) {
  return postRoomArchive(spaceRef, 'archive', options.idempotencyKey)
}

/** Bring an archived room back (owner or manager). */
export function unarchiveSpaceRoom(spaceRef: string, options: { readonly idempotencyKey?: string } = {}) {
  return postRoomArchive(spaceRef, 'unarchive', options.idempotencyKey)
}
