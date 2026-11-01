// WS07-S12: the room's artifact aggregate. One page of summaries of the agent
// artifacts persisted in this Space's threads (latest version per artifact).
// Content is never in this response: the viewer reads the thread through the
// transcript route, which asks Control for the reader's own decision.
//
// Wire (plan WS07, API paragraph):
//   GET /api/v1/spaces/{ref}/artifacts?limit&cursor
//   -> { data: { artifacts: [...], next_cursor, shared_read: {available}, retention: {zdr} } }
// Errors are the typed `{error:{code,message,details}}` envelope and arrive as
// an ApiError (403 = the reader may not read other members' turns here).
import {
  MAX_ROOM_ARTIFACT_TITLE_CHARS,
  ROOM_ARTIFACT_KINDS,
  type RoomArtifactKind,
} from '@/features/spaces/lib/room-artifacts'
import { requestJson } from '../http'

/** Entries the page keeps per response; the server clamps to 200 itself. */
export const SPACE_ARTIFACT_PAGE_SIZE = 50

export type SpaceArtifactSummary = {
  readonly thread_id: string
  readonly message_id: string
  readonly artifact_id: string
  readonly kind: RoomArtifactKind
  readonly title: string
  /** The latest persisted revision of this artifact. */
  readonly version: number
  readonly version_count: number
  readonly size_chars: number
  readonly agent_name?: string
  readonly created_at?: string
}

export type SpaceArtifactPage = {
  readonly artifacts: readonly SpaceArtifactSummary[]
  /** Null on the last page. */
  readonly nextCursor: string | null
  /** False when the organization has not enabled shared reads for this Space. */
  readonly sharedReadAvailable: boolean
  /** True: an organization with zero data retention keeps no artifacts. Null: unknown. */
  readonly zdr: boolean | null
}

function nonEmptyString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}

function count(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) && value >= 0 ? Math.floor(value) : fallback
}

/**
 * Summaries through the same kind and size gate as the transcript mapper: a
 * kind the room cannot draw (`pdf`, `image`, `binary`, unknown) is not listed,
 * so no entry can lead to a viewer branch the room does not allow.
 */
function artifactSummaries(value: unknown): SpaceArtifactSummary[] {
  if (!Array.isArray(value)) return []
  const out: SpaceArtifactSummary[] = []
  for (const entry of value) {
    if (!entry || typeof entry !== 'object') continue
    const record = entry as Record<string, unknown>
    const threadId = nonEmptyString(record.thread_id)
    const artifactId = nonEmptyString(record.artifact_id)
    const kind = record.kind
    if (!threadId || !artifactId) continue
    if (typeof kind !== 'string' || !(ROOM_ARTIFACT_KINDS as readonly string[]).includes(kind)) continue
    const version = record.version
    if (typeof version !== 'number' || !Number.isInteger(version) || version < 1) continue
    const title = typeof record.title === 'string' ? record.title.trim() : ''
    if (title.length > MAX_ROOM_ARTIFACT_TITLE_CHARS) continue
    const agentName = nonEmptyString(record.agent_name)
    const createdAt = nonEmptyString(record.created_at)
    out.push({
      thread_id: threadId,
      message_id: nonEmptyString(record.message_id) ?? '',
      artifact_id: artifactId,
      kind: kind as RoomArtifactKind,
      title,
      version,
      version_count: Math.max(1, count(record.version_count, 1)),
      size_chars: count(record.size_chars, 0),
      ...(agentName ? { agent_name: agentName } : {}),
      ...(createdAt ? { created_at: createdAt } : {}),
    })
  }
  return out
}

export async function getSpaceArtifacts(
  spaceRef: string,
  options: { readonly limit?: number; readonly cursor?: string } = {},
): Promise<SpaceArtifactPage> {
  const query = new URLSearchParams()
  query.set('limit', String(options.limit ?? SPACE_ARTIFACT_PAGE_SIZE))
  if (options.cursor) query.set('cursor', options.cursor)
  const response = await requestJson<{
    artifacts?: unknown
    next_cursor?: unknown
    shared_read?: { available?: unknown } | null
    retention?: { zdr?: unknown } | null
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/artifacts?${query.toString()}`)
  const zdr = response.retention?.zdr
  return {
    artifacts: artifactSummaries(response.artifacts),
    nextCursor: nonEmptyString(response.next_cursor) ?? null,
    // Absent means the server did not say it was unavailable.
    sharedReadAvailable: response.shared_read?.available !== false,
    zdr: typeof zdr === 'boolean' ? zdr : null,
  }
}
