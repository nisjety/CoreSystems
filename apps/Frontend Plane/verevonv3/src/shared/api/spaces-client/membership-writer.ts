// WS04-S6: room member calls on the Control writer. The gateway answers 404 `feature_disabled`
// when its writer flag is off; Control's typed errors (`room_role_forbidden`,
// `room_elevated_role_requires_owner`, `room_sole_owner`, `room_member_changed` with
// `details.current_revision`, ...) arrive as an ApiError carrying the code, rendered by
// `translateApiError` from the WS04 banner of shared/i18n/errors.ts.
import { requestJson } from '../http'

/** Change one member's role. `expectedRevision` is the membership revision the caller saw. */
export async function changeSpaceMemberRole(
  spaceRef: string,
  memberId: string,
  role: string,
  expectedRevision?: number,
): Promise<{ role: string; fromRole: string; changed: boolean }> {
  const response = await requestJson<{ role?: string; from_role?: string; changed?: boolean }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/members/${encodeURIComponent(memberId)}`,
    {
      method: 'PATCH',
      body: JSON.stringify(
        expectedRevision === undefined ? { role } : { role, expected_revision: expectedRevision },
      ),
    },
  )
  return {
    role: response.role ?? role,
    fromRole: response.from_role ?? '',
    changed: response.changed === true,
  }
}

/** Leave a room as yourself (the literal `me`). Control refuses the last owner. */
export async function leaveSpaceRoom(
  spaceRef: string,
): Promise<{ changed: boolean; left: boolean; memberCount: number }> {
  const response = await requestJson<{ changed?: boolean; left?: boolean; member_count?: number }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/members/me`,
    { method: 'DELETE' },
  )
  return {
    changed: response.changed === true,
    left: response.left === true,
    memberCount: response.member_count ?? 0,
  }
}

// WS04-S8: join an open room and set a room's visibility. The gateway answers 404
// `feature_disabled` with `SPACE_ROOM_DIRECTORY_API` off. Join: 404 `room_not_found` (a private
// room, a personal Space, another organization's room and an unknown ref are the same answer),
// 409 `room_archived`. Settings: 403 `room_role_forbidden`, 403 `directory_disabled` (the
// organization's directory policy is off), 409 `room_roster_derived` / `room_archived`, 400
// `invalid_request`. All arrive as an ApiError carrying the code.
export type RoomVisibility = 'private' | 'org_public'
export type RoomJoinRole = 'viewer' | 'editor'

export type SpaceJoinResult = {
  role: string
  /** False when the caller was already a member (a repeated join is not an error). */
  changed: boolean
  alreadyMember: boolean
  /** Always false right after a join: poll the membership read until the audience is ready. */
  recipientReady: boolean
}

/** Join an open room from the directory. The role is whatever the room set for self-join. */
export async function joinSpaceRoom(spaceRef: string, idempotencyKey?: string): Promise<SpaceJoinResult> {
  const response = await requestJson<{
    role?: string | null
    changed?: boolean
    already_member?: boolean
    recipient_ready?: boolean
  }>(`/api/v1/spaces/${encodeURIComponent(spaceRef)}/join`, {
    method: 'POST',
    body: JSON.stringify(idempotencyKey ? { idempotency_key: idempotencyKey } : {}),
  })
  return {
    role: typeof response.role === 'string' ? response.role : '',
    changed: response.changed === true,
    alreadyMember: response.already_member === true,
    recipientReady: response.recipient_ready === true,
  }
}

export type SpaceRoomSettingsResult = {
  visibility: RoomVisibility | null
  joinRole: RoomJoinRole | null
  changed: boolean
}

/** Set a room's visibility and/or the role a self-join receives. Send only what changes. */
export async function updateSpaceRoomSettings(
  spaceRef: string,
  settings: { readonly visibility?: RoomVisibility; readonly joinRole?: RoomJoinRole },
  idempotencyKey?: string,
): Promise<SpaceRoomSettingsResult> {
  const body: Record<string, string> = {}
  if (settings.visibility !== undefined) body.visibility = settings.visibility
  if (settings.joinRole !== undefined) body.join_role = settings.joinRole
  if (idempotencyKey) body.idempotency_key = idempotencyKey
  const response = await requestJson<{ visibility?: string | null; join_role?: string | null; changed?: boolean }>(
    `/api/v1/spaces/${encodeURIComponent(spaceRef)}/settings`,
    { method: 'PATCH', body: JSON.stringify(body) },
  )
  return {
    visibility:
      response.visibility === 'org_public' ? 'org_public' : response.visibility === 'private' ? 'private' : null,
    joinRole: response.join_role === 'editor' ? 'editor' : response.join_role === 'viewer' ? 'viewer' : null,
    changed: response.changed === true,
  }
}
