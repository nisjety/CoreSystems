// WS04-S3: people search for the room member picker. `GET /api/v1/directory/people` answers
// 404 `feature_disabled` with the gateway flag `SPACE_PEOPLE_SEARCH_API` off, 403
// `active_membership_required` without a live organization membership, 400 `invalid_request`
// with `details.{field,reason}` (q over 100 chars, limit over 50, bad cursor), 429
// `rate_limited` and 503 `directory_unavailable`. They arrive as an ApiError carrying the code,
// rendered by `translateApiError` from the WS04 banner of shared/i18n/errors.ts. The
// organization and the caller come from the session, never from this call.
import { createSignal } from 'solid-js'
import { requestJson } from '../http'

export type DirectoryPerson = {
  /** The person's Control subject id: what a room grant carries. */
  subjectId: string
  displayName: string
  email: string
  isSelf: boolean
}

export type DirectoryPeoplePage = {
  people: readonly DirectoryPerson[]
  /** Opaque keyset cursor for the next page; `null` when there is no more. */
  nextCursor: string | null
}

export type SearchDirectoryPeopleOptions = {
  /** Page size; the server caps it at 50 (default 20). */
  readonly limit?: number
  /** `next_cursor` of the previous page. */
  readonly cursor?: string | null
  /**
   * A room whose current members are left out of the result. The server honours it only when
   * the caller is a member of that room, and ignores it otherwise.
   */
  readonly excludeMemberOf?: string
  readonly signal?: AbortSignal
}

type RawPerson = { subject_id?: unknown; display_name?: unknown; email?: unknown; is_self?: unknown }

/** Search the organization's people. An empty `q` browses the directory from the top. */
export async function searchDirectoryPeople(
  q: string,
  options: SearchDirectoryPeopleOptions = {},
): Promise<DirectoryPeoplePage> {
  const params = new URLSearchParams()
  const query = q.trim()
  if (query) params.set('q', query)
  if (options.limit !== undefined) params.set('limit', String(options.limit))
  if (options.cursor) params.set('cursor', options.cursor)
  if (options.excludeMemberOf) params.set('exclude_member_of', options.excludeMemberOf)
  const qs = params.toString()
  const response = await requestJson<{ people?: readonly RawPerson[]; next_cursor?: string | null }>(
    `/api/v1/directory/people${qs ? `?${qs}` : ''}`,
    options.signal ? { signal: options.signal } : undefined,
  )
  const people: DirectoryPerson[] = []
  for (const raw of response.people ?? []) {
    if (typeof raw.subject_id !== 'string' || raw.subject_id === '') continue
    people.push({
      subjectId: raw.subject_id,
      displayName: typeof raw.display_name === 'string' ? raw.display_name : '',
      email: typeof raw.email === 'string' ? raw.email : '',
      isSelf: raw.is_self === true,
    })
  }
  return {
    people,
    nextCursor: typeof response.next_cursor === 'string' && response.next_cursor !== '' ? response.next_cursor : null,
  }
}

// WS04-S8: the room directory. `GET /api/v1/directory/rooms` answers 404 `feature_disabled` with
// the gateway flag `SPACE_ROOM_DIRECTORY_API` off, 403 for a caller without an organization
// membership, 400 `invalid_request` for a bad cursor, 429 `rate_limited` and 503
// `directory_unavailable` (never an empty list: a failed read is not "no rooms"). With the
// organization's directory policy off it answers `enabled:false` and no rooms (200), which is a
// different fact from an empty directory and is rendered as such.
export type DirectoryRoom = {
  spaceRef: string
  name: string
  topic: string | null
  memberCount: number
  /** The role a self-join receives: never manager or owner. */
  joinRole: 'viewer' | 'editor'
}

export type DirectoryRoomsPage = {
  /** False when the organization has not turned the directory on. */
  enabled: boolean
  rooms: readonly DirectoryRoom[]
  nextCursor: string | null
}

export type ListDirectoryRoomsOptions = {
  /** Page cursor (`nextCursor` of the previous page). */
  readonly cursor?: string | null
  readonly signal?: AbortSignal
}

type RawRoom = {
  space_ref?: unknown
  name?: unknown
  topic?: unknown
  member_count?: unknown
  join_role?: unknown
}

/** Browse the organization's open rooms, 25 per page, sorted by name. `q` filters name and topic. */
export async function listDirectoryRooms(
  q: string,
  options: ListDirectoryRoomsOptions = {},
): Promise<DirectoryRoomsPage> {
  const params = new URLSearchParams()
  const query = q.trim()
  if (query) params.set('q', query)
  if (options.cursor) params.set('cursor', options.cursor)
  const qs = params.toString()
  const response = await requestJson<{
    enabled?: unknown
    rooms?: readonly RawRoom[]
    next_cursor?: string | null
  }>(`/api/v1/directory/rooms${qs ? `?${qs}` : ''}`, options.signal ? { signal: options.signal } : undefined)
  const rooms: DirectoryRoom[] = []
  for (const raw of response.rooms ?? []) {
    if (typeof raw.space_ref !== 'string' || raw.space_ref === '') continue
    if (typeof raw.name !== 'string' || raw.name.trim() === '') continue
    rooms.push({
      spaceRef: raw.space_ref,
      name: raw.name,
      topic: typeof raw.topic === 'string' && raw.topic.trim() !== '' ? raw.topic : null,
      memberCount: typeof raw.member_count === 'number' ? raw.member_count : 0,
      joinRole: raw.join_role === 'editor' ? 'editor' : 'viewer',
    })
  }
  return {
    enabled: response.enabled === true,
    rooms,
    nextCursor: typeof response.next_cursor === 'string' && response.next_cursor !== '' ? response.next_cursor : null,
  }
}

// Whether the gateway offers the room directory, as the last `GET /api/v1/spaces` said
// (`features.room_directory`). A rollout answer only, off until a listing says otherwise; the
// directory route itself answers 404 `feature_disabled` when the flag is off.
const [roomDirectoryListed, setRoomDirectoryListedSignal] = createSignal(false)

export function setRoomDirectoryListed(enabled: boolean): void {
  setRoomDirectoryListedSignal(enabled)
}

/** Reactive: true once the Spaces listing reported `features.room_directory`. */
export function spaceRoomDirectoryListed(): boolean {
  return roomDirectoryListed()
}

// WS04-S9: the composed people-and-agent directory and the resolver. Both answers are PRESENCE,
// never authority: that someone appears here says only that they are in the organization (or that
// an agent definition is published), not that they may be added, mentioned or given anything. The
// gateway marks them `meta.presence_only:true`; the client carries the same fact as a constant so a
// caller cannot mistake a hit for permission. `GET /api/v1/directory/people?kinds=` and
// `POST /api/v1/directory/resolve` answer 404 `feature_disabled` with the gateway flag
// `SPACE_DIRECTORY_RESOLVER_API` off, 400 `invalid_request` with `details.{field,reason}`, 429
// `rate_limited` and 503 `directory_unavailable` (the resolver instead answers 200 `none` with
// reason `directory_unavailable`: an outage is never "no such person").
export type DirectoryAgent = {
  /** Stable reference of the published agent definition. Not a Control subject id. */
  agentRef: string
  displayName: string
  description: string
  /** Rooms the CALLER is in that have this agent installed. Never another room's name or ref. */
  installedIn: readonly string[]
}

export type DirectoryEntry =
  | ({ kind: 'person' } & DirectoryPerson)
  | ({ kind: 'agent' } & DirectoryAgent)

export type DirectoryEntryKind = 'person' | 'agent'

export type SearchDirectoryOptions = {
  /** Which kinds to list; defaults to both. */
  readonly kinds?: readonly DirectoryEntryKind[]
  readonly limit?: number
  readonly cursor?: string | null
  /** Same meaning as for people search: honoured by the server only when the caller is in that room. */
  readonly excludeMemberOf?: string
  readonly signal?: AbortSignal
}

export type DirectorySearchPage = {
  results: readonly DirectoryEntry[]
  nextCursor: string | null
  /** Always true: a directory hit is presence, not authority. */
  presenceOnly: true
}

type RawEntry = {
  kind?: unknown
  subject_id?: unknown
  display_name?: unknown
  email?: unknown
  is_self?: unknown
  agent_ref?: unknown
  description?: unknown
  installed_in?: unknown
}

function entryFromRaw(raw: RawEntry): DirectoryEntry | null {
  if (raw.kind === 'agent') {
    if (typeof raw.agent_ref !== 'string' || raw.agent_ref === '') return null
    return {
      kind: 'agent',
      agentRef: raw.agent_ref,
      displayName: typeof raw.display_name === 'string' ? raw.display_name : '',
      description: typeof raw.description === 'string' ? raw.description : '',
      installedIn: Array.isArray(raw.installed_in)
        ? raw.installed_in.filter((ref): ref is string => typeof ref === 'string' && ref !== '')
        : [],
    }
  }
  if (typeof raw.subject_id !== 'string' || raw.subject_id === '') return null
  return {
    kind: 'person',
    subjectId: raw.subject_id,
    displayName: typeof raw.display_name === 'string' ? raw.display_name : '',
    email: typeof raw.email === 'string' ? raw.email : '',
    isSelf: raw.is_self === true,
  }
}

/**
 * Search people and published agents together. People come in pages (the cursor), agents only on
 * the first page (the server caps them), so a caller that pages must not expect agents again.
 */
export async function searchDirectory(q: string, options: SearchDirectoryOptions = {}): Promise<DirectorySearchPage> {
  const params = new URLSearchParams()
  const query = q.trim()
  if (query) params.set('q', query)
  params.set('kinds', (options.kinds && options.kinds.length > 0 ? options.kinds : ['person', 'agent']).join(','))
  if (options.limit !== undefined) params.set('limit', String(options.limit))
  if (options.cursor) params.set('cursor', options.cursor)
  if (options.excludeMemberOf) params.set('exclude_member_of', options.excludeMemberOf)
  const response = await requestJson<{ results?: readonly RawEntry[]; next_cursor?: string | null }>(
    `/api/v1/directory/people?${params.toString()}`,
    options.signal ? { signal: options.signal } : undefined,
  )
  const results: DirectoryEntry[] = []
  for (const raw of response.results ?? []) {
    const entry = entryFromRaw(raw)
    if (entry) results.push(entry)
  }
  return {
    results,
    nextCursor: typeof response.next_cursor === 'string' && response.next_cursor !== '' ? response.next_cursor : null,
    presenceOnly: true,
  }
}

export type ResolveDirectoryKind = 'person' | 'agent' | 'any'

export type ResolveDirectoryInput = {
  /** A name, a unique name prefix, or an exact email. 1..100 characters. */
  readonly query: string
  /** Defaults to `any`. */
  readonly kind?: ResolveDirectoryKind
  /**
   * Scope the answer to this room's members. The server honours it only when the caller is in
   * the room (otherwise it is ignored, so this cannot probe a room's roster).
   */
  readonly spaceRef?: string
  readonly signal?: AbortSignal
}

export type DirectoryResolveReason = 'no_match' | 'not_in_room' | 'directory_unavailable'

export type DirectoryResolution =
  | { status: 'ok'; match: DirectoryEntry; presenceOnly: true }
  | { status: 'ambiguous'; candidates: readonly DirectoryEntry[]; presenceOnly: true }
  | { status: 'none'; reason: DirectoryResolveReason; presenceOnly: true }

function resolveReason(value: unknown): DirectoryResolveReason {
  return value === 'not_in_room' || value === 'directory_unavailable' ? value : 'no_match'
}

/**
 * Turn what someone typed into one directory entry: `ok` (exact email, exact full name or a unique
 * prefix), `ambiguous` (up to five candidates to choose from) or `none`. A malformed or unknown
 * status reads as `none`, never as a match.
 */
export async function resolveDirectory(input: ResolveDirectoryInput): Promise<DirectoryResolution> {
  const body: Record<string, string> = { query: input.query.trim(), kind: input.kind ?? 'any' }
  if (input.spaceRef) body.space_ref = input.spaceRef
  const response = await requestJson<{
    status?: unknown
    match?: RawEntry | null
    candidates?: readonly RawEntry[]
    reason?: unknown
  }>('/api/v1/directory/resolve', {
    method: 'POST',
    body: JSON.stringify(body),
    ...(input.signal ? { signal: input.signal } : {}),
  })
  if (response.status === 'ok' && response.match) {
    const match = entryFromRaw(response.match)
    if (match) return { status: 'ok', match, presenceOnly: true }
  }
  if (response.status === 'ambiguous') {
    const candidates: DirectoryEntry[] = []
    for (const raw of response.candidates ?? []) {
      const entry = entryFromRaw(raw)
      if (entry) candidates.push(entry)
    }
    if (candidates.length > 0) return { status: 'ambiguous', candidates: candidates.slice(0, 5), presenceOnly: true }
  }
  return { status: 'none', reason: resolveReason(response.reason), presenceOnly: true }
}
