// WS13-S4a: entry references. `GET /api/v1/spaces/{space_ref}/entries/{message_id}` answers
// `{data:{kind:"message",space_ref,thread_id,message_id}}`; 403 `space_read_not_authorized` and 404
// `entry_not_found` (identical for hidden, missing and foreign) arrive as an ApiError carrying the code.
// `GET /api/v1/spaces/features` answers `{data:{features:{...}}}`. Both are served by the gateway's
// message-search slice (WS13-S3); until it lands they 404, which callers treat as "feature off".
import { requestJson } from '../http'

export type SpaceEntryMessage = {
  readonly kind: 'message'
  readonly spaceRef: string
  readonly threadId: string
  readonly messageId: string
}

/** Resolve a message id to the thread that holds it. Fails closed: anything unreadable throws. */
export async function resolveSpaceEntry(spaceRef: string, messageId: string): Promise<SpaceEntryMessage> {
  const response = await requestJson<Record<string, unknown>>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/entries/${encodeURIComponent(messageId)}`,
  )
  const threadId = typeof response.thread_id === 'string' ? response.thread_id.trim() : ''
  const resolved = typeof response.message_id === 'string' ? response.message_id.trim() : ''
  if (response.kind !== 'message' || !threadId || !resolved) {
    throw new Error('invalid_space_entry_response')
  }
  return { kind: 'message', spaceRef, threadId, messageId: resolved }
}

export type SpaceFeatureFlags = Readonly<Record<string, boolean>>

/** The BFF's rollout flags (`message_search`, `entry_links`, `command_palette`, `room_pins`, `thread_fork`). */
export async function getSpaceFeatures(): Promise<SpaceFeatureFlags> {
  const response = await requestJson<{ features?: unknown }>('/api/v1/spaces/features')
  const raw = response.features
  const flags: Record<string, boolean> = {}
  if (raw && typeof raw === 'object') {
    for (const [key, value] of Object.entries(raw as Record<string, unknown>)) flags[key] = value === true
  }
  return flags
}
