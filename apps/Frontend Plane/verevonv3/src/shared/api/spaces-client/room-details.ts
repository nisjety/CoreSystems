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

// WS04-S7: archive and unarchive run through the `spaces.archive_room` / `spaces.unarchive_room` action contract
// (shared/actions/domains/spaces/ws04.ts, features/spaces/lib/space-room-archive.ts), not a direct route call. The
// gateway's `data` is `{state, changed, archived_at}`; refusals (`room_role_forbidden`, `room_archived`,
// `room_not_archivable`, `room_state_conflict`, `room_personal_space`, `rate_limited`, `room_admin_unavailable`)
// arrive as the action's typed error.
export type SpaceRoomArchiveResult = {
  /** The room's state after the call. */
  state: 'archived' | 'active'
  /** False when the room was already in that state (a repeated call is not an error). */
  changed: boolean
  /** When the room was archived, as the server's string; `null` once unarchived. */
  archivedAt: string | null
}

const INVALID_ROOM_ARCHIVE_REPLY = 'invalid_room_archive_response'

/**
 * WS04-S7: reads the result of `spaces.archive_room` and `spaces.unarchive_room`, which is the gateway's `data`:
 * `{state, changed, archived_at}`. Anything outside that shape throws, so the caller shows its error and does not
 * claim a state it did not read.
 */
export function spaceRoomArchiveFrom(value: unknown): SpaceRoomArchiveResult {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(INVALID_ROOM_ARCHIVE_REPLY)
  const row = value as Record<string, unknown>
  const rawState = row.state
  const state = rawState === 'archived' || rawState === 'active' ? rawState : null
  const changed = row.changed
  const archivedAt = row.archived_at
  if (state === null || typeof changed !== 'boolean') throw new Error(INVALID_ROOM_ARCHIVE_REPLY)
  if (archivedAt !== null && archivedAt !== undefined && typeof archivedAt !== 'string') {
    throw new Error(INVALID_ROOM_ARCHIVE_REPLY)
  }
  return {
    state,
    changed,
    archivedAt: typeof archivedAt === 'string' && archivedAt !== '' ? archivedAt : null,
  }
}
