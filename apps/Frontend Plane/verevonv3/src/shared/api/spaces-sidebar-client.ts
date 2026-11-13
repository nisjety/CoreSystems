// WS16-S9: typed client for the personal Spaces sidebar document.
//
// `GET /api/v1/spaces/sidebar` -> { mode: 'server' | 'zdr_local', revision, doc | null, limits }.
// Errors arrive as an ApiError carrying the code: 404 `space_sidebar_disabled` (flag off: the caller
// selects the legacy panel), 403 `active_membership_required`, 503 `space_sidebar_unavailable`.
//
// `PUT /api/v1/spaces/sidebar { base_revision, doc }` -> { revision, doc }. Errors: 400
// `invalid_sidebar_doc` (details.reason), 409 `space_sidebar_conflict` (details { revision, doc } to
// rebase on), 412 `zdr_durable_mutation_forbidden` (the organization keeps no durable state: the
// layout stays in this browser), 413, 404, 403, 503.
//
// The organization and the user come from the session on the server; this client names neither.
import { ApiError, requestJson } from './http'
import { parseSidebarDoc, type SidebarDoc } from '@/shared/actions/space-sidebar'

export type SpaceSidebarMode = 'server' | 'zdr_local'

export type SpaceSidebarSnapshot = {
  readonly mode: SpaceSidebarMode
  /** 0 when the person has saved nothing yet (and always under `zdr_local`). */
  readonly revision: number
  /** Null when nothing is stored. A stored document that does not parse is treated as nothing. */
  readonly doc: SidebarDoc | null
}

export type SpaceSidebarSaved = { readonly revision: number; readonly doc: SidebarDoc | null }

/** The stored revision and document a 409 carries, for the store to rebase on. */
export type SpaceSidebarConflict = { readonly revision: number; readonly doc: SidebarDoc | null }

export const SPACE_SIDEBAR_PATH = '/api/v1/spaces/sidebar'

function revisionOf(value: unknown): number {
  return typeof value === 'number' && Number.isInteger(value) && value >= 0 ? value : 0
}

export async function getSpaceSidebar(): Promise<SpaceSidebarSnapshot> {
  const response = await requestJson<{ mode?: unknown; revision?: unknown; doc?: unknown }>(SPACE_SIDEBAR_PATH)
  return {
    mode: response.mode === 'zdr_local' ? 'zdr_local' : 'server',
    revision: revisionOf(response.revision),
    doc: response.doc == null ? null : parseSidebarDoc(response.doc),
  }
}

export async function putSpaceSidebar(baseRevision: number, doc: SidebarDoc): Promise<SpaceSidebarSaved> {
  const response = await requestJson<{ revision?: unknown; doc?: unknown }>(SPACE_SIDEBAR_PATH, {
    method: 'PUT',
    body: JSON.stringify({ base_revision: baseRevision, doc }),
  })
  return { revision: revisionOf(response.revision), doc: response.doc == null ? null : parseSidebarDoc(response.doc) }
}

/** The stored layout a 409 `space_sidebar_conflict` carries, or null when the error is another one. */
export function sidebarConflictFrom(error: unknown): SpaceSidebarConflict | null {
  if (!(error instanceof ApiError) || error.status !== 409 || error.code !== 'space_sidebar_conflict') return null
  const details = error.details
  if (!details || Array.isArray(details)) return { revision: 0, doc: null }
  return {
    revision: revisionOf(details.revision),
    doc: details.doc == null ? null : parseSidebarDoc(details.doc),
  }
}
