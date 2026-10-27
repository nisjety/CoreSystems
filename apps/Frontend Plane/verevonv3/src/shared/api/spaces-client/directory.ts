// WS04-S3: people search for the room member picker. `GET /api/v1/directory/people` answers
// 404 `feature_disabled` with the gateway flag `SPACE_PEOPLE_SEARCH_API` off, 403
// `active_membership_required` without a live organization membership, 400 `invalid_request`
// with `details.{field,reason}` (q over 100 chars, limit over 50, bad cursor), 429
// `rate_limited` and 503 `directory_unavailable`. They arrive as an ApiError carrying the code,
// rendered by `translateApiError` from the WS04 banner of shared/i18n/errors.ts. The
// organization and the caller come from the session, never from this call.
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
