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
