// WS19-S5: the room canvas from the SPA, the one shared Markdown document of a Space. The gateway
// (apps/gateway/src/domains/spaces/canvas.rs) relays Data Plane's answers under the caller's own Control
// decision, and that decision never reaches the browser. Presentation-free here: the gateway and the owning
// planes decide who may read or write, and a refusal arrives as an ApiError carrying its typed code.
//   GET  /api/v1/spaces/{ref}/canvas?known_version_id=      {data:{canvas, head, can_write}}, or {data:{unchanged, head_version_id}}
//   GET  /api/v1/spaces/{ref}/canvas/versions?limit&cursor  {data:[version without text, current], meta:{limit, next_cursor, window?}}
//   GET  /api/v1/spaces/{ref}/canvas/versions/{version_id}  {data:version with its text}
// Writes are not here. A save and a restore go through the action registry (`spaces.save_canvas`,
// `spaces.restore_canvas_version`, see features/spaces/lib/space-canvas-save.ts), and their answer is read by
// `spaceCanvasWriteResultFrom`. A Space with no canvas yet answers canvas and head null: an empty room is a state,
// not an error.
import { clientError, requestJson, requestJsonPage } from '../http'

/** Who wrote a version: a member, or an agent run acting for the member who asked for it. */
export type SpaceCanvasAuthorKind = 'user' | 'agent'
export type SpaceCanvasChangeKind = 'create' | 'edit' | 'restore'

/** The canvas itself, without text. `headVersionId` is null only before its first version. */
export type SpaceCanvas = {
  readonly canvasId: string
  readonly spaceRef: string
  readonly state: 'active' | 'archived'
  readonly headVersionId: string | null
  readonly headVersionNo: number
  readonly createdAt: string
  readonly updatedAt: string
}

/**
 * One version. `content` is present where the route answers one version with its text (the head, or a single
 * version read); a history page carries none. `authorId` is a member's subject id, or `run:<run id>` for an agent.
 * Show a name from the roster, never the id.
 */
export type SpaceCanvasVersion = {
  readonly versionId: string
  readonly versionNo: number
  readonly parentVersionId: string | null
  readonly authorKind: SpaceCanvasAuthorKind
  readonly authorId: string
  /** For an agent run: the member the run acted for. */
  readonly onBehalfOf: string | null
  readonly runId: string | null
  readonly changeKind: SpaceCanvasChangeKind
  readonly restoredFromVersionId: string | null
  readonly summary: string | null
  readonly contentBytes: number
  readonly contentSha256: string
  readonly createdAt: string
  readonly content?: string
}

/**
 * The answer to a read: the head with its text, `unchanged` for the head the caller already holds, or no canvas
 * yet (`canvas` and `head` null). `canWrite` is informational: Control and Data Plane enforce every write.
 */
export type SpaceCanvasRead =
  | { readonly unchanged: true; readonly headVersionId: string }
  | {
      readonly unchanged: false
      readonly canvas: SpaceCanvas | null
      readonly head: SpaceCanvasVersion | null
      readonly canWrite: boolean
    }

/** A save or a restore: a new version, or the head already holds this text and nothing was stored. */
export type SpaceCanvasWriteResult =
  | { readonly unchanged: false; readonly canvasId: string; readonly version: SpaceCanvasVersion; readonly deduplicated: boolean }
  | { readonly unchanged: true; readonly canvasId: string; readonly headVersionId: string }

/**
 * A member's history window on the canvas (D21m): the history lists only versions from `startsAt` on, plus the current
 * head wherever it falls, and `hiddenBefore` says older versions exist that the window withholds. Absent for a member
 * who sees the whole history. Restoring a version is refused under any window (Control), so a windowed history offers
 * comparison only.
 */
export type SpaceCanvasHistoryWindow = {
  readonly startsAt: string | null
  readonly hiddenBefore: boolean
}

/** One page of history, newest first. `nextCursor` names the next older page, or is null at the end. */
export type SpaceCanvasVersionPage = {
  readonly versions: readonly SpaceCanvasVersion[]
  readonly nextCursor: string | null
  readonly window: SpaceCanvasHistoryWindow | null
}

type Wire = Readonly<Record<string, unknown>>

function wireRecord(value: unknown): Wire | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Wire) : null
}

function text(value: unknown): string | null {
  return typeof value === 'string' && value !== '' ? value : null
}

function count(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function canvasFrom(wire: Wire | null): SpaceCanvas | null {
  if (!wire) return null
  const canvasId = text(wire.canvas_id)
  const spaceRef = text(wire.space_ref)
  const createdAt = text(wire.created_at)
  const updatedAt = text(wire.updated_at)
  if (!canvasId || !spaceRef || !createdAt || !updatedAt) return null
  return {
    canvasId,
    spaceRef,
    state: wire.state === 'archived' ? 'archived' : 'active',
    headVersionId: text(wire.head_version_id),
    headVersionNo: count(wire.head_version_no) ?? 0,
    createdAt,
    updatedAt,
  }
}

function versionFrom(wire: Wire | null): SpaceCanvasVersion | null {
  if (!wire) return null
  const versionId = text(wire.version_id)
  const versionNo = count(wire.version_no)
  const authorId = text(wire.author_id)
  const createdAt = text(wire.created_at)
  if (!versionId || versionNo === null || !authorId || !createdAt) return null
  const content = typeof wire.content === 'string' ? wire.content : undefined
  return {
    versionId,
    versionNo,
    parentVersionId: text(wire.parent_version_id),
    authorKind: wire.author_kind === 'agent' ? 'agent' : 'user',
    authorId,
    onBehalfOf: text(wire.on_behalf_of),
    runId: text(wire.run_id),
    changeKind: wire.change_kind === 'restore' ? 'restore' : wire.change_kind === 'edit' ? 'edit' : 'create',
    restoredFromVersionId: text(wire.restored_from_version_id),
    summary: text(wire.summary),
    contentBytes: count(wire.content_bytes) ?? 0,
    contentSha256: text(wire.content_sha256) ?? '',
    createdAt,
    ...(content !== undefined ? { content } : {}),
  }
}

function spaceCanvasPath(spaceRef: string): string {
  return `/api/v1/spaces/${encodeURIComponent(spaceRef)}/canvas`
}

/**
 * The room's canvas as this person may read it. Pass the head you hold as `knownVersionId` to ask only whether
 * it moved: the gateway then answers `unchanged` with no text. A malformed canvas answer is an outage, never an
 * empty room.
 */
export async function getSpaceCanvas(spaceRef: string, knownVersionId?: string | null): Promise<SpaceCanvasRead> {
  const query = knownVersionId ? `?known_version_id=${encodeURIComponent(knownVersionId)}` : ''
  const data = wireRecord(await requestJson<unknown>(`${spaceCanvasPath(spaceRef)}${query}`))
  if (data?.unchanged === true) {
    return { unchanged: true, headVersionId: text(data.head_version_id) ?? knownVersionId ?? '' }
  }
  const canvasWire = data?.canvas
  const headWire = data?.head
  const hasCanvas = canvasWire !== null && canvasWire !== undefined
  const canvas = hasCanvas ? canvasFrom(wireRecord(canvasWire)) : null
  const head = headWire !== null && headWire !== undefined ? versionFrom(wireRecord(headWire)) : null
  if (hasCanvas && (canvas === null || head === null)) throw clientError(503, 'canvas_unavailable')
  return { unchanged: false, canvas, head, canWrite: data?.can_write === true }
}

/** The answer of a save or a restore action: a stored version (201), or the unchanged head (200). */
export function spaceCanvasWriteResultFrom(body: unknown): SpaceCanvasWriteResult {
  const wire = wireRecord(body)
  const canvasId = text(wire?.canvas_id)
  if (!canvasId) throw clientError(503, 'canvas_unavailable')
  if (wire?.unchanged === true) {
    return { unchanged: true, canvasId, headVersionId: text(wire.head_version_id) ?? '' }
  }
  const version = versionFrom(wireRecord(wire?.version))
  if (!version) throw clientError(503, 'canvas_unavailable')
  return { unchanged: false, canvasId, version, deduplicated: wire?.deduplicated === true }
}

/**
 * One page of the canvas history, newest first, without text. `requestJsonPage` keeps the envelope's `meta`, which
 * names the next older page (`meta.next_cursor`, a number on the wire and a digit string here). A row that is not a
 * version fails the whole page: a history that cannot be read in full is an outage, never a shorter list.
 */
export async function listSpaceCanvasVersions(
  spaceRef: string,
  options: { readonly limit?: number; readonly cursor?: string | null } = {},
): Promise<SpaceCanvasVersionPage> {
  const search = new URLSearchParams()
  if (options.limit !== undefined) search.set('limit', String(options.limit))
  if (options.cursor) search.set('cursor', options.cursor)
  const query = search.toString()
  const { data, meta } = await requestJsonPage<unknown>(`${spaceCanvasPath(spaceRef)}/versions${query ? `?${query}` : ''}`)
  const rows: readonly unknown[] = Array.isArray(data) ? data : []
  const versions: SpaceCanvasVersion[] = []
  for (const row of rows) {
    const version = versionFrom(wireRecord(row))
    if (!version) throw clientError(503, 'canvas_unavailable')
    versions.push(version)
  }
  const next = meta?.next_cursor
  const nextCursor = typeof next === 'number' && Number.isFinite(next) ? String(next) : text(next)
  const windowWire = wireRecord(meta?.window)
  const window = windowWire
    ? { startsAt: text(windowWire.starts_at), hiddenBefore: windowWire.hidden_before === true }
    : null
  return { versions, nextCursor, window }
}

/** One version with its text, for the comparison and the restore confirmation. */
export async function getSpaceCanvasVersion(spaceRef: string, versionId: string): Promise<SpaceCanvasVersion> {
  const version = versionFrom(
    wireRecord(await requestJson<unknown>(`${spaceCanvasPath(spaceRef)}/versions/${encodeURIComponent(versionId)}`)),
  )
  if (!version) throw clientError(503, 'canvas_unavailable')
  return version
}
