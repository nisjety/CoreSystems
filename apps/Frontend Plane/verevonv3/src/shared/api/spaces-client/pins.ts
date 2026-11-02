// WS13-S8: room pins. `PUT` and `DELETE /api/v1/spaces/{space_ref}/pins/{thread_id}` answer 404
// `feature_disabled` with the gateway flag off, 403 `room_role_forbidden` for a viewer, 403
// `space_read_not_authorized`, 404 `thread_not_found` / `space_not_found`, 409 `room_pins_limit`,
// 503 `room_pins_unavailable`. They arrive as an ApiError carrying the code. A pin is a set of
// identifiers: the thread listing carries `pins`, and the timeline lifts those posts.
import { requestJson } from '../http'

/** One room pin as the thread listing reports it. `pinned_at` is epoch milliseconds. */
export type SpaceRoomPin = {
  readonly thread_id: string
  /** The pinner's identity id; resolve it to a name through the roster, never show it raw. */
  readonly pinned_by: string
  readonly pinned_at: number
}

/** Keeps only well-formed pins; anything else (absent, malformed) is "no pins". */
export function spacePinsFrom(raw: unknown): readonly SpaceRoomPin[] {
  if (!Array.isArray(raw)) return []
  const pins: SpaceRoomPin[] = []
  for (const item of raw) {
    if (!item || typeof item !== 'object') continue
    const value = item as Record<string, unknown>
    if (
      typeof value.thread_id === 'string' &&
      value.thread_id !== '' &&
      typeof value.pinned_by === 'string' &&
      typeof value.pinned_at === 'number' &&
      Number.isFinite(value.pinned_at)
    ) {
      pins.push({ thread_id: value.thread_id, pinned_by: value.pinned_by, pinned_at: value.pinned_at })
    }
  }
  return pins
}

/** Pin a post for the whole room (editor or above). Idempotent. */
export async function pinSpaceThread(spaceRef: string, threadId: string): Promise<{ pinned_at: number | null }> {
  const response = await requestJson<{ pinned_at?: unknown }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/pins/${encodeURIComponent(threadId)}`,
    { method: 'PUT' },
  )
  return { pinned_at: typeof response.pinned_at === 'number' ? response.pinned_at : null }
}

/** Remove a room pin (editor or above). Idempotent. */
export async function unpinSpaceThread(spaceRef: string, threadId: string): Promise<void> {
  await requestJson<unknown>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/pins/${encodeURIComponent(threadId)}`,
    { method: 'DELETE' },
  )
}
