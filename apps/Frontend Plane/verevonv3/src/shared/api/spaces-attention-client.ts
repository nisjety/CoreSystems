// WS11-S4: the cross-room attention client.
//
// Wire (plan WS11, API paragraph; the gateway half is WS11-S3's Frontend side):
//   GET  /api/v1/spaces/attention?refs=a,b           -> { data: { generated_at, mode, rooms[], totals, truncated }, meta: { stale_sources } }
//   POST /api/v1/spaces/attention/read-all           {space_refs?, kinds?} -> { data: { rooms_marked, items_resolved, marked_at } }
//   POST /api/v1/spaces/{ref}/unread                 {before_ms, idempotency_key} -> { data: { last_read_at, manual_unread } }
//   GET|PUT /api/v1/spaces/{ref}/notify              {level, hide_preview?} -> { data: { level, hide_preview } }
//
// Everything here is ids, closed words and timestamps. The `badge` of a room is computed ONCE,
// in the gateway (Rust, table-tested); this client passes it through and never derives one. A
// shape the client does not understand is dropped (the room is simply absent), never guessed at.
import { requestJson } from './http'

export type AttentionBadgeKind = 'count' | 'dot' | 'none'
export type AttentionBadge = { readonly kind: AttentionBadgeKind; readonly n: number }
export type SpaceNotifyLevel = 'all' | 'mentions' | 'off'

export type AttentionRoom = {
  readonly spaceRef: string
  readonly unreadThreads: number
  readonly unreadCapped: boolean
  readonly mentions: number
  readonly needsYou: number
  readonly working: number
  readonly lastActivityAt: number | null
  readonly lastReadAt: number | null
  readonly manualUnread: boolean
  readonly level: SpaceNotifyLevel
  readonly badge: AttentionBadge
  /** Sources the gateway could not read for this room; a gap is "unknown", never "zero". */
  readonly gaps: readonly string[]
}

export type AttentionTotals = {
  /** The sum of room `n` only. Unread is reported apart and is never added. */
  readonly badge: number
  readonly unreadRooms: number
  readonly otherOpen: number
}

export type SpacesAttention = {
  readonly generatedAt: number | null
  /** `zdr_empty`: the org keeps nothing durable, so there is nothing to count. */
  readonly mode: 'live' | 'zdr_empty'
  readonly rooms: readonly AttentionRoom[]
  readonly totals: AttentionTotals
  readonly truncated: boolean
  readonly staleSources: readonly string[]
}

type Wire = Record<string, unknown>

const isRecord = (value: unknown): value is Wire => !!value && typeof value === 'object' && !Array.isArray(value)
const count = (value: unknown): number =>
  typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.floor(value) : 0
const millis = (value: unknown): number | null =>
  typeof value === 'number' && Number.isFinite(value) ? value : null
const words = (value: unknown): readonly string[] =>
  Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string').slice(0, 20) : []

export function isNotifyLevel(value: unknown): value is SpaceNotifyLevel {
  return value === 'all' || value === 'mentions' || value === 'off'
}

function badgeFrom(value: unknown): AttentionBadge {
  if (!isRecord(value)) return { kind: 'none', n: 0 }
  const kind = value.kind
  if (kind === 'count') {
    const n = count(value.n)
    return n > 0 ? { kind: 'count', n } : { kind: 'none', n: 0 }
  }
  if (kind === 'dot') return { kind: 'dot', n: 0 }
  return { kind: 'none', n: 0 }
}

function roomFrom(value: unknown): AttentionRoom | undefined {
  if (!isRecord(value)) return undefined
  const spaceRef = typeof value.space_ref === 'string' ? value.space_ref.trim() : ''
  if (!spaceRef) return undefined
  return {
    spaceRef,
    unreadThreads: count(value.unread_threads),
    unreadCapped: value.unread_capped === true,
    mentions: count(value.mentions),
    needsYou: count(value.needs_you),
    working: count(value.working),
    lastActivityAt: millis(value.last_activity_at),
    lastReadAt: millis(value.last_read_at),
    manualUnread: value.manual_unread === true,
    level: isNotifyLevel(value.level) ? value.level : 'mentions',
    badge: badgeFrom(value.badge),
    gaps: words(value.gaps),
  }
}

/** The pure parse of the aggregate body, exported for its test. `meta` is the envelope's sibling of `data`. */
export function attentionFromWire(data: unknown, meta?: unknown): SpacesAttention {
  const body = isRecord(data) ? data : {}
  const totals = isRecord(body.totals) ? body.totals : {}
  const rooms = (Array.isArray(body.rooms) ? body.rooms : [])
    .map(roomFrom)
    .filter((room): room is AttentionRoom => room !== undefined)
  return {
    generatedAt: millis(body.generated_at),
    mode: body.mode === 'zdr_empty' ? 'zdr_empty' : 'live',
    rooms,
    totals: {
      badge: count(totals.badge),
      unreadRooms: count(totals.unread_rooms),
      otherOpen: count(totals.other_open),
    },
    truncated: body.truncated === true,
    staleSources: isRecord(meta) ? words(meta.stale_sources) : [],
  }
}

/**
 * The room-by-room counts for the caller, in one polled request. `refs` is optional (the gateway
 * intersects with Control's index and caps at 60); pass it only to narrow.
 *
 * `requestJson` unwraps `{data}` and drops `meta`, so the aggregate is read through the same
 * transport and `stale_sources` is carried inside `data.stale_sources` when the gateway mirrors
 * it there; otherwise the per-room `gaps` already say which source was unknown.
 */
export async function getSpacesAttention(refs?: readonly string[]): Promise<SpacesAttention> {
  const query = refs && refs.length > 0 ? `?refs=${encodeURIComponent(refs.slice(0, 60).join(','))}` : ''
  const data = await requestJson<unknown>(`/api/v1/spaces/attention${query}`)
  const stale = isRecord(data) ? { stale_sources: data.stale_sources } : undefined
  return attentionFromWire(data, stale)
}

/**
 * The shell rollout flag `attention` from `GET /api/v1/spaces/features` (the gateway sets it to
 * `attention_api && attention_ui`). `true` or `false` as the gateway stated it; `undefined` when the
 * route did not answer or names no `attention` key (an older gateway), which callers treat as
 * "unknown" and leave to the aggregate's own 404 `space_attention_disabled`. Presentation only,
 * never an authorization input: the gateway re-checks the flag on every attention route.
 */
export async function getSpacesAttentionFeature(): Promise<boolean | undefined> {
  try {
    const response = await requestJson<{ features?: unknown }>('/api/v1/spaces/features')
    const features = isRecord(response) ? response.features : undefined
    if (!isRecord(features) || !('attention' in features)) return undefined
    return features.attention === true
  } catch {
    return undefined
  }
}

export type MarkAllReadResult = {
  readonly roomsMarked: number
  readonly itemsResolved: number
  readonly markedAt: number | null
}

/** "Mark all read": advances existing markers and resolves read-driven items; never touches "needs you". */
export async function markAllSpacesRead(input: {
  readonly spaceRefs?: readonly string[]
  readonly kinds?: readonly ('mention' | 'reply')[]
} = {}): Promise<MarkAllReadResult> {
  const wire = await requestJson<Wire>('/api/v1/spaces/attention/read-all', {
    method: 'POST',
    body: JSON.stringify({
      ...(input.spaceRefs ? { space_refs: input.spaceRefs.slice(0, 60) } : {}),
      ...(input.kinds ? { kinds: input.kinds } : {}),
    }),
  })
  return {
    roomsMarked: count(wire?.rooms_marked),
    itemsResolved: count(wire?.items_resolved),
    markedAt: millis(wire?.marked_at),
  }
}

function idempotencyKey(prefix: string): string {
  const random =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto ? crypto.randomUUID() : `${Date.now()}-${Math.random()}`
  return `${prefix}-${random}`.slice(0, 64)
}

export type MarkUnreadResult = { readonly lastReadAt: number | null; readonly manualUnread: boolean }

/** Move the room's read position back to just before `beforeMs` (epoch ms). Never moves it forward. */
export async function markSpaceUnread(spaceRef: string, beforeMs: number): Promise<MarkUnreadResult> {
  const wire = await requestJson<Wire>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/unread`, {
    method: 'POST',
    body: JSON.stringify({ before_ms: Math.floor(beforeMs), idempotency_key: idempotencyKey('unread') }),
  })
  return { lastReadAt: millis(wire?.last_read_at), manualUnread: wire?.manual_unread !== false }
}

export type SpaceNotifySettings = { readonly level: SpaceNotifyLevel; readonly hidePreview: boolean }

export async function getSpaceNotify(spaceRef: string): Promise<SpaceNotifySettings> {
  const wire = await requestJson<Wire>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/notify`)
  return { level: isNotifyLevel(wire?.level) ? wire.level : 'mentions', hidePreview: wire?.hide_preview === true }
}

/** The caller's own notification level for one room. A closed vocabulary, allowed under ZDR. */
export async function setSpaceNotifyLevel(
  spaceRef: string,
  level: SpaceNotifyLevel,
  hidePreview?: boolean,
): Promise<SpaceNotifySettings> {
  const wire = await requestJson<Wire>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/notify`, {
    method: 'PUT',
    body: JSON.stringify({ level, ...(hidePreview === undefined ? {} : { hide_preview: hidePreview }) }),
  })
  return { level: isNotifyLevel(wire?.level) ? wire.level : level, hidePreview: wire?.hide_preview === true }
}
