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
