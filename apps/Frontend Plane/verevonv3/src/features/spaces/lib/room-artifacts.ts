/**
 * Agent artifacts as the room reads them (WS07-S11).
 *
 * A room transcript carries, on an assistant turn, whatever Model Plane
 * persisted under `metadata.artifacts` (`{id, kind, title, content, version}`).
 * Room data is shared with other people and was never typed by the viewer, so
 * this mapper is the single gate between the wire and the viewer: only the
 * persisted kinds, only text within size caps, only well-formed entries. A
 * `pdf`, `image` or `binary` entry can therefore never reach the unsandboxed PDF
 * iframe or the image branch of `ArtifactViewer` from room data.
 *
 * Pure and JSX-free: the shared API client imports it, so it must not pull the
 * chat renderer in. The merge into viewer items (`roomItems`) lives next to it
 * in `room-artifact-items.ts` for the same reason.
 *
 * Nothing here touches browser storage. Room artifacts are held in memory only.
 */
import type { ChatArtifact } from '@/features/chat/components/chat-types'

/** Exactly the kinds Model Plane persists for a turn. */
export const ROOM_ARTIFACT_KINDS = ['document', 'code', 'html'] as const
export type RoomArtifactKind = (typeof ROOM_ARTIFACT_KINDS)[number]

export const MAX_ROOM_ARTIFACT_TITLE_CHARS = 200
export const MAX_ROOM_ARTIFACT_CONTENT_CHARS = 200_000
/** A turn never legitimately carries more than a handful; bounds a hostile payload. */
export const MAX_ROOM_ARTIFACTS_PER_TURN = 50

function isRoomArtifactKind(value: unknown): value is RoomArtifactKind {
  return typeof value === 'string' && (ROOM_ARTIFACT_KINDS as readonly string[]).includes(value)
}

/**
 * The well-formed room artifacts in `value` (a turn's `artifacts` metadata).
 * Keeps entries with a non-empty string `id`, a kind in {@link ROOM_ARTIFACT_KINDS},
 * a string `title` (trimmed, at most 200 chars), a string `content` (at most
 * 200,000 chars) and an integer `version >= 1`. Everything else is dropped.
 */
export function roomArtifactsFromMetadata(value: unknown): ChatArtifact[] {
  if (!Array.isArray(value)) return []
  const artifacts: ChatArtifact[] = []
  for (const entry of value) {
    if (artifacts.length >= MAX_ROOM_ARTIFACTS_PER_TURN) break
    if (!entry || typeof entry !== 'object') continue
    const record = entry as Record<string, unknown>
    const id = typeof record.id === 'string' ? record.id.trim() : ''
    if (!id || !isRoomArtifactKind(record.kind)) continue
    if (typeof record.title !== 'string' || typeof record.content !== 'string') continue
    const title = record.title.trim()
    if (title.length > MAX_ROOM_ARTIFACT_TITLE_CHARS) continue
    if (record.content.length > MAX_ROOM_ARTIFACT_CONTENT_CHARS) continue
    const version = record.version
    if (typeof version !== 'number' || !Number.isInteger(version) || version < 1) continue
    artifacts.push({ id, kind: record.kind, title, content: record.content, version })
  }
  return artifacts
}

/**
 * One live `artifact` stream event from the sender's own turn, through the same
 * gate as persisted data. `undefined` when the event is not a room artifact.
 */
export function roomArtifactFromEvent(event: {
  id?: string
  kind?: string
  title?: string
  content?: string
  version?: number
}): ChatArtifact | undefined {
  return roomArtifactsFromMetadata([event])[0]
}

/**
 * Build-time switch for the room artifact surfaces until the BFF delivers
 * `features.room_artifacts` (WS07-S9). On in dev, off in production unless
 * `VITE_SPACE_ROOM_ARTIFACTS_UI=true`; `=false` turns it off in dev too. A
 * rollout control only, never an authorization input.
 */
export function roomArtifactsEnabled(): boolean {
  const flag = import.meta.env.VITE_SPACE_ROOM_ARTIFACTS_UI
  if (flag === 'true') return true
  if (flag === 'false') return false
  return import.meta.env.DEV === true
}
