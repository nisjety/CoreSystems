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
