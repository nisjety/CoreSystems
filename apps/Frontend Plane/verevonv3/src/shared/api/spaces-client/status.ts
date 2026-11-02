// WS11-S10: org-scoped member status and agent availability chips.
//
// Wire (gateway: apps/gateway/src/domains/spaces/status.rs; flag `space_status_api`):
//   GET /api/v1/spaces/status                         -> { data: { state, text, expires_at } }  (all null when none set)
//   PUT /api/v1/spaces/status {state, text?, expires_at?} -> { data: { state, text, expires_at } }
//   GET /api/v1/spaces/{ref}/roster   user members carry `status` ({state, text, expires_at} | null)
//   GET /api/v1/spaces/{ref}/agents   agents carry `activity` ('working' | 'waiting' | 'idle')
//
// Errors arrive as an ApiError carrying the code: 404 `space_status_disabled` (flag off, hide the
// control), 400 `invalid_status`, 403 `active_membership_required`, 412
// `zdr_durable_mutation_forbidden` (text under Zero Data Retention; a state-only write is allowed),
// 503 `status_unavailable`. The status belongs to one organization and is never shown in another.
// The text is the one free-text field: it is rendered as plain text only and never fed to a model.
// `spaces.set_status` is the action contract (shared/actions/domains/spaces/ws11.ts).
import { requestJson } from '../http'

export type SpaceMemberStatusState = 'available' | 'away' | 'dnd' | 'offline'
export type SpaceAgentActivity = 'working' | 'waiting' | 'idle'

export const SPACE_STATUS_STATES: readonly SpaceMemberStatusState[] = ['available', 'away', 'dnd', 'offline']
/** The most characters of status text the server accepts. */
export const SPACE_STATUS_TEXT_MAX = 80

export type SpaceMemberStatus = {
  readonly state: SpaceMemberStatusState
  /** Absent under Zero Data Retention (the server withholds it) or when none was set. */
  readonly text: string | null
  /** Epoch milliseconds, or null for "until changed". An expired status never reaches the client. */
  readonly expiresAt: number | null
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  !!value && typeof value === 'object' && !Array.isArray(value)

export function isSpaceStatusState(value: unknown): value is SpaceMemberStatusState {
  return value === 'available' || value === 'away' || value === 'dnd' || value === 'offline'
}

export function isSpaceAgentActivity(value: unknown): value is SpaceAgentActivity {
  return value === 'working' || value === 'waiting' || value === 'idle'
}

/**
 * Reads one wire status. `null` and anything that is not a well-formed status (unknown state,
 * wrong types) read as "no status": a chip is never guessed at.
 */
export function spaceStatusFrom(raw: unknown): SpaceMemberStatus | null {
  if (!isRecord(raw) || !isSpaceStatusState(raw.state)) return null
  const text = typeof raw.text === 'string' && raw.text.trim() !== '' ? raw.text : null
  const expiresAt = typeof raw.expires_at === 'number' && Number.isFinite(raw.expires_at) ? raw.expires_at : null
  return { state: raw.state, text, expiresAt }
}

/** The caller's own status in their organization, or null when none is set (or it expired). */
export async function getOwnSpaceStatus(): Promise<SpaceMemberStatus | null> {
  const response = await requestJson<unknown>('/api/v1/spaces/status')
  return spaceStatusFrom(response)
}

export type SetSpaceStatusInput = {
  readonly state: SpaceMemberStatusState
  /** At most 80 characters; omit or leave blank for no text. Refused under Zero Data Retention. */
  readonly text?: string
  /** Epoch milliseconds in the future, at most seven days ahead. Omit for until changed. */
  readonly expiresAt?: number
}

/** Replaces the caller's own status. The previous text and expiry do not carry over. */
export async function setOwnSpaceStatus(input: SetSpaceStatusInput): Promise<SpaceMemberStatus | null> {
  const text = input.text?.trim()
  const body: Record<string, unknown> = { state: input.state }
  if (text) body.text = text
  if (typeof input.expiresAt === 'number') body.expires_at = input.expiresAt
  const response = await requestJson<unknown>('/api/v1/spaces/status', {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  return spaceStatusFrom(response)
}
