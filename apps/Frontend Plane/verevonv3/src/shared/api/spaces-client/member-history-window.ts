// D18: per-member room history visibility. Control owns the rule, the audit row and the floor every read
// signs; the gateway relays it (apps/gateway/src/domains/spaces/room_admin.rs). A window can only HIDE: it
// narrows what an already-authorised member reads and never widens anything.
//
// `GET` and `PUT /api/v1/spaces/{space_ref}/members/{subject_id}/history-window` answer 404 `feature_disabled`
// while the gateway's `member_history_windows` switch is off, 403 `room_role_forbidden` /
// `room_delegation_required` for anyone but a room owner or manager, 404 `room_not_found` /
// `member_not_found`, 409 `room_archived` / `room_roster_derived` / `room_member_changed`
// (`details.current_revision`), 422 `member_history_window_invalid` (`details.field` is `mode` or `since_at`),
// 429 `rate_limited`, 503 `member_history_window_unavailable` / `space_authority_unavailable` /
// `room_admin_unavailable`, 400 `invalid_request`. All arrive as an ApiError carrying the code, worded by
// `translateApiError` (shared/i18n/errors.ts).
//
// The member's OWN window rides the membership read (`membership.history_window`, `source`, `mode`,
// `since_at`, `hidden`) for that member alone; `ownHistoryWindowFrom` reads it.
import { ApiError, requestJson } from '../http'

/** The modes an admin may set on one member. `room` is not one of them: only an invite writes it, inside Control. */
export const MEMBER_HISTORY_MODES = ['full', 'since_date', 'since_join'] as const
export type MemberHistoryMode = (typeof MEMBER_HISTORY_MODES)[number]

export function isMemberHistoryMode(value: unknown): value is MemberHistoryMode {
  return typeof value === 'string' && (MEMBER_HISTORY_MODES as readonly string[]).includes(value)
}

/**
 * One member's window as Control reports it to a room owner or manager. `source` is `member` when the member's
 * own setting decides and `room` when the room's history mode does (then `mode` names the room's mode). The
 * words are not checked against the vocabulary, so a mode Control adds later is not an outage: `mode` keeps the
 * open tail and a caller that offers a choice must test it with {@link isMemberHistoryMode}. `revision` is what
 * an `expectedRevision` compares against.
 */
export type SpaceMemberHistoryWindow = {
  readonly source: 'member' | 'room' | (string & {})
  readonly mode: MemberHistoryMode | (string & {})
  /** An RFC 3339 instant, or `null` when the window names none. */
  readonly sinceAt: string | null
  readonly revision: number
}

/** The wire shape of the caller's own window on the membership read (`membership.history_window`). Absent on an older Control. */
export type SpaceMembershipHistoryWindow = {
  readonly source?: string
  readonly mode?: string
  readonly since_at?: string | null
  readonly hidden?: boolean
}

/**
 * The window the signed-in member reads under, for that member alone. `hidden` is Control's own statement that
 * the floor hides earlier messages from this member, derived from the same resolution every read signs, so it
 * never names a window where no read hides anything.
 */
export type SpaceOwnHistoryWindow = {
  readonly source: 'member' | 'room' | (string & {})
  readonly mode: MemberHistoryMode | (string & {})
  readonly sinceAt: string | null
  readonly hidden: boolean
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

/** A plain word (a source or a mode). Anything else is not a window. */
function wordOf(value: unknown): string | undefined {
  return typeof value === 'string' && /^[a-z][a-z0-9_]{0,63}$/.test(value) ? value : undefined
}

/** An absent or null instant is `null`; a string that is not a time is `undefined`, which makes the whole window unreadable. */
function instantOf(value: unknown): string | null | undefined {
  if (value === undefined || value === null) return null
  if (typeof value !== 'string') return undefined
  const text = value.trim()
  return text !== '' && Number.isFinite(Date.parse(text)) ? text : undefined
}

/**
 * Reads one member's window from the wire. A body that is not a window is `undefined`, never a made-up one:
 * the person would act on it. A missing `revision` is 0 (a room-sourced window with no row of its own).
 */
export function memberHistoryWindowFrom(raw: unknown): SpaceMemberHistoryWindow | undefined {
  if (!isRecord(raw)) return undefined
  const source = wordOf(raw.source)
  const mode = wordOf(raw.mode)
  const sinceAt = instantOf(raw.since_at)
  if (source === undefined || mode === undefined || sinceAt === undefined) return undefined
  const revision = raw.revision === undefined || raw.revision === null ? 0 : raw.revision
  if (typeof revision !== 'number' || !Number.isInteger(revision) || revision < 0) return undefined
  return { source, mode, sinceAt, revision }
}

/**
 * Reads the caller's own window from `membership.history_window`. Absent, malformed or without a boolean
 * `hidden` is `undefined`, which draws nothing: a marker is shown only on Control's own `hidden: true`.
 */
export function ownHistoryWindowFrom(raw: unknown): SpaceOwnHistoryWindow | undefined {
  if (!isRecord(raw)) return undefined
  const source = wordOf(raw.source)
  const mode = wordOf(raw.mode)
  const sinceAt = instantOf(raw.since_at)
  if (source === undefined || mode === undefined || sinceAt === undefined || typeof raw.hidden !== 'boolean') {
    return undefined
  }
  return { source, mode, sinceAt, hidden: raw.hidden }
}

export type SpaceMemberHistoryWindowRead = {
  readonly spaceRef: string
  readonly subjectId: string
  readonly window: SpaceMemberHistoryWindow
}

export type SpaceMemberHistoryWindowChange = SpaceMemberHistoryWindowRead & {
  /** False when the member already read under that window (a repeated change is not an error). */
  readonly changed: boolean
}

type WindowAnswer = { space_ref?: unknown; subject_id?: unknown; changed?: unknown; history_window?: unknown }

function windowPath(spaceRef: string, memberId: string): string {
  return `/api/v1/spaces/${encodeURIComponent(spaceRef)}/members/${encodeURIComponent(memberId)}/history-window`
}

/** A success that is not a window is the upstream layer's own failure, the answer the gateway gives for the same case. */
function unreadableAnswer(): ApiError {
  return new ApiError('The answer was not a history window.', 502, 'upstream_unavailable')
}

function readOf(answer: WindowAnswer, spaceRef: string, memberId: string): SpaceMemberHistoryWindowRead {
  const window = memberHistoryWindowFrom(answer.history_window)
  if (!window) throw unreadableAnswer()
  return {
    spaceRef: typeof answer.space_ref === 'string' && answer.space_ref !== '' ? answer.space_ref : spaceRef,
    subjectId: typeof answer.subject_id === 'string' && answer.subject_id !== '' ? answer.subject_id : memberId,
    window,
  }
}

/**
 * Read one member's window (room owner or manager). A member with no window of their own reads under the
 * room's mode (`source: 'room'`). Not cached: the revision it carries is what a later change compares against.
 */
export async function getSpaceMemberHistoryWindow(
  spaceRef: string,
  memberId: string,
): Promise<SpaceMemberHistoryWindowRead> {
  const answer = await requestJson<WindowAnswer>(windowPath(spaceRef, memberId), {
    method: 'GET',
    cache: 'no-store',
  })
  return readOf(answer, spaceRef, memberId)
}

/**
 * Set one member's window (room owner or manager). `since_date` carries `sinceAt`, an RFC 3339 instant that is
 * not in the future (Control answers 422 `member_history_window_invalid` for a missing or future one); the other
 * two modes never carry one, so a stray `sinceAt` is dropped here rather than sent. `expectedRevision` is the
 * revision the caller last read: a stale one answers 409 `room_member_changed` with `details.current_revision`.
 */
export async function setSpaceMemberHistoryWindow(
  spaceRef: string,
  memberId: string,
  change: {
    readonly mode: MemberHistoryMode
    readonly sinceAt?: string
    readonly expectedRevision?: number
  },
): Promise<SpaceMemberHistoryWindowChange> {
  if (!isMemberHistoryMode(change.mode)) {
    throw new TypeError('The history mode must be full, since_join or since_date.')
  }
  const body: Record<string, unknown> = { mode: change.mode }
  if (change.mode === 'since_date' && change.sinceAt !== undefined) body.since_at = change.sinceAt
  if (change.expectedRevision !== undefined) body.expected_revision = change.expectedRevision
  const answer = await requestJson<WindowAnswer>(windowPath(spaceRef, memberId), {
    method: 'PUT',
    body: JSON.stringify(body),
  })
  return { ...readOf(answer, spaceRef, memberId), changed: answer.changed === true }
}
