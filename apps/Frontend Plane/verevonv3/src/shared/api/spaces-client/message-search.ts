// WS13-S5: in-room message search. `POST /api/v1/spaces/{space_ref}/search` carries the query in the
// body (never a URL) and answers `Cache-Control: no-store`:
//   -> 200 { data: { hits: [{ message_id, thread_id, space_id, role, author_subject_id?, agent_name?,
//                             created_at, snippet: [{ text, matched }], thread_title }],
//                    next_cursor, truncated, coverage ("shared" | "own_threads_only"), excluded: [] } }
// Errors are the typed `{error:{code,message,details}}` envelope and arrive as an ApiError:
// 400 `invalid_query` (details `{min_chars,max_chars}`) / `invalid_filter` / `invalid_cursor`,
// 403 `active_membership_required`, 404 `space_not_found` / `space_search_disabled`,
// 429 `search_busy`, 502 `invalid_model_gateway_response`,
// 503 `space_lifecycle_unavailable` / `space_decision_unavailable` / `delegated_auth_unavailable` /
// `search_timeout` / `search_unavailable`. The copy for each code lives in shared/i18n/errors.ts.
//
// Nothing here stores anything: no cache, no memo, no history. The caller owns the AbortSignal.
import { requestJson } from '../http'

export type SpaceSearchScope = 'room' | 'thread' | 'person'

/** `shared`: the room's shared record was searched. `own_threads_only`: only the caller's own threads. */
export type SpaceSearchCoverage = 'shared' | 'own_threads_only'

export type SpaceSearchRequest = {
  readonly query: string
  /** True while typing (the last word matches as a prefix); false for a finished word. */
  readonly prefix: boolean
  readonly scope: SpaceSearchScope
  /** Required by the gateway for scope `thread`. */
  readonly threadId?: string
  /** Required by the gateway for scope `person`. */
  readonly authorSubjectId?: string
  readonly limit?: number
  readonly cursor?: string
}

export type SpaceSearchSnippetSegment = {
  readonly text: string
  readonly matched: boolean
}

export type SpaceSearchHit = {
  readonly messageId: string
  readonly threadId: string
  readonly spaceId: string
  readonly role: string
  readonly authorSubjectId: string | undefined
  readonly agentName: string | undefined
  /** RFC 3339, as the server stated it. */
  readonly createdAt: string
  readonly snippet: readonly SpaceSearchSnippetSegment[]
  readonly threadTitle: string
}

export type SpaceSearchPage = {
  readonly hits: readonly SpaceSearchHit[]
  readonly nextCursor: string | null
  readonly truncated: boolean
  readonly coverage: SpaceSearchCoverage
  readonly excluded: readonly string[]
}

/** The most snippet text kept per hit, so a hostile response cannot balloon the list. */
const SNIPPET_TEXT_MAX_CHARS = 600

function text(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined
}

function snippetFrom(raw: unknown): readonly SpaceSearchSnippetSegment[] {
  if (!Array.isArray(raw)) return []
  const segments: SpaceSearchSnippetSegment[] = []
  let budget = SNIPPET_TEXT_MAX_CHARS
  for (const item of raw) {
    if (budget <= 0) break
    if (!item || typeof item !== 'object') continue
    const value = item as Record<string, unknown>
    if (typeof value.text !== 'string' || value.text === '') continue
    const part = value.text.length > budget ? value.text.slice(0, budget) : value.text
    budget -= part.length
    segments.push({ text: part, matched: value.matched === true })
  }
  return segments
}

function hitFrom(raw: unknown): SpaceSearchHit | null {
  if (!raw || typeof raw !== 'object') return null
  const value = raw as Record<string, unknown>
  const messageId = text(value.message_id)
  const threadId = text(value.thread_id)
  const spaceId = text(value.space_id)
  if (!messageId || !threadId || !spaceId) return null
  return {
    messageId,
    threadId,
    spaceId,
    role: text(value.role) ?? 'user',
    authorSubjectId: text(value.author_subject_id),
    agentName: text(value.agent_name),
    createdAt: text(value.created_at) ?? '',
    snippet: snippetFrom(value.snippet),
    threadTitle: typeof value.thread_title === 'string' ? value.thread_title : '',
  }
}

/** Reads the wire page. A body that is not a page (no hits array, unknown coverage) throws: fail closed. */
export function spaceSearchPageFrom(raw: unknown): SpaceSearchPage {
  const value = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>
  if (!Array.isArray(value.hits) || (value.coverage !== 'shared' && value.coverage !== 'own_threads_only')) {
    throw new Error('invalid_space_search_response')
  }
  const hits: SpaceSearchHit[] = []
  for (const item of value.hits) {
    const hit = hitFrom(item)
    if (hit) hits.push(hit)
  }
  return {
    hits,
    nextCursor: text(value.next_cursor) ?? null,
    truncated: value.truncated === true,
    coverage: value.coverage,
    excluded: Array.isArray(value.excluded)
      ? value.excluded.filter((item): item is string => typeof item === 'string')
      : [],
  }
}

/** Search one room's messages. Pass an AbortSignal and abort it when the query changes. */
export async function searchSpaceMessages(
  spaceRef: string,
  request: SpaceSearchRequest,
  signal?: AbortSignal,
): Promise<SpaceSearchPage> {
  const body: Record<string, unknown> = {
    query: request.query,
    prefix: request.prefix,
    scope: request.scope,
  }
  if (request.scope === 'thread' && request.threadId) body.thread_id = request.threadId
  if (request.scope === 'person' && request.authorSubjectId) body.author_subject_id = request.authorSubjectId
  if (request.limit !== undefined) body.limit = request.limit
  if (request.cursor) body.cursor = request.cursor
  const response = await requestJson<unknown>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/search`, {
    method: 'POST',
    body: JSON.stringify(body),
    signal,
    cache: 'no-store',
  })
  return spaceSearchPageFrom(response)
}
