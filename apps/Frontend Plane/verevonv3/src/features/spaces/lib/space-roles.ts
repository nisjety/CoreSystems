// Room role rules (WS04-S6): a pure mirror of Control's `internal/spaces/roles.go`.
//
// Control decides every add, role change and removal under the caller's signed delegation and
// re-reads every input inside its own transaction. This mirror only decides which controls to
// OFFER, so a person is not shown a door Control would refuse. It is checked against the vendored
// golden `src/shared/api/contracts/space-room-role-matrix.v1.json`, which the Go side
// copies byte for byte (scripts/tests/space-contract-vectors-sync.test.sh). A divergence here can
// only hide or show a button; it can never grant anything.

export type RoomRole = 'viewer' | 'editor' | 'manager' | 'owner'

export const ROOM_ROLES: readonly RoomRole[] = ['viewer', 'editor', 'manager', 'owner']

export type RoleDenialCode =
  | 'room_role_forbidden'
  | 'room_elevated_role_requires_owner'
  | 'room_sole_owner'
  | 'room_not_joinable'

const RANK: Readonly<Record<RoomRole, number>> = { viewer: 1, editor: 2, manager: 3, owner: 4 }

export function isRoomRole(value: unknown): value is RoomRole {
  return typeof value === 'string' && Object.prototype.hasOwnProperty.call(RANK, value)
}

export function roleRank(role: string): number {
  return isRoomRole(role) ? RANK[role] : 0
}

export function isElevatedRole(role: string): boolean {
  return roleRank(role) >= RANK.manager
}

/** Add: rank >= manager, and a new elevated role needs an owner. */
export function canAdd(actorRole: string, newRole: string): RoleDenialCode | null {
  if (roleRank(actorRole) < RANK.manager) return 'room_role_forbidden'
  if (isElevatedRole(newRole) && actorRole !== 'owner') return 'room_elevated_role_requires_owner'
  return null
}

/**
 * Change a role: a self-demotion (or a no-op on yourself) is always allowed and a self-promotion
 * never is; changing someone else needs manager or above, and neither the target's current role
 * nor the new role may be elevated unless the actor is an owner. The last owner cannot be
 * demoted.
 */
export function canChangeRole(
  actorRole: string,
  targetCurrent: string,
  newRole: string,
  targetIsActor: boolean,
  activeOwners: number,
): RoleDenialCode | null {
  if (targetIsActor) {
    if (roleRank(newRole) > roleRank(targetCurrent)) {
      return isElevatedRole(newRole) ? 'room_elevated_role_requires_owner' : 'room_role_forbidden'
    }
    if (targetCurrent === 'owner' && newRole !== 'owner' && activeOwners <= 1) return 'room_sole_owner'
    return null
  }
  if (roleRank(actorRole) < RANK.manager) return 'room_role_forbidden'
  if ((isElevatedRole(targetCurrent) || isElevatedRole(newRole)) && actorRole !== 'owner') {
    return 'room_elevated_role_requires_owner'
  }
  if (targetCurrent === 'owner' && newRole !== 'owner' && activeOwners <= 1) return 'room_sole_owner'
  return null
}

/** Remove or leave: leaving is always allowed except for the last owner. */
export function canRemove(
  actorRole: string,
  targetRole: string,
  targetIsActor: boolean,
  activeOwners: number,
): RoleDenialCode | null {
  if (!targetIsActor) {
    if (roleRank(actorRole) < RANK.manager) return 'room_role_forbidden'
    if (isElevatedRole(targetRole) && actorRole !== 'owner') return 'room_elevated_role_requires_owner'
  }
  if (targetRole === 'owner' && activeOwners <= 1) return 'room_sole_owner'
  return null
}

/** A self-join role is viewer or editor, never elevated. */
export function canJoin(joinRole: string): RoleDenialCode | null {
  return joinRole === 'viewer' || joinRole === 'editor' ? null : 'room_not_joinable'
}

/** Roles the actor may offer when adding a person. */
export function assignableRolesForAdd(actorRole: string): readonly RoomRole[] {
  return ROOM_ROLES.filter((role) => canAdd(actorRole, role) === null)
}

/** Roles the actor may set on an existing member (the member's current role is always listed). */
export function assignableRolesForChange(
  actorRole: string,
  targetCurrent: string,
  targetIsActor: boolean,
  activeOwners: number,
): readonly RoomRole[] {
  return ROOM_ROLES.filter(
    (role) =>
      role === targetCurrent
      || canChangeRole(actorRole, targetCurrent, role, targetIsActor, activeOwners) === null,
  )
}

/** Active human owners in a roster, as Control counts them for the sole-owner rule. */
export function countActiveOwners(
  roster: readonly { readonly subject_type: string; readonly role: string }[],
): number {
  return roster.filter((member) => member.subject_type === 'user' && member.role === 'owner').length
}
