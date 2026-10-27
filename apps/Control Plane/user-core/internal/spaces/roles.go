package spaces

// Room role rules (WS04-S4). Pure functions: no database, no clock. Every
// input is read by the caller inside the transaction that holds the Space row
// lock, never taken from the gateway. Rank: viewer 1 < editor 2 < manager 3 <
// owner 4. A role is "elevated" when it is manager or owner.

const (
	RoleViewer  = "viewer"
	RoleEditor  = "editor"
	RoleManager = "manager"
	RoleOwner   = "owner"
)

// Stable error codes of the room writer. They are part of the gateway and SPA
// contract; never rename one.
const (
	CodeRoomNotFound              = "room_not_found"
	CodeRoomRoleForbidden         = "room_role_forbidden"
	CodeRoomElevatedNeedsOwner    = "room_elevated_role_requires_owner"
	CodeRoomSoleOwner             = "room_sole_owner"
	CodeRoomArchived              = "room_archived"
	CodeRoomRosterDerived         = "room_roster_derived"
	CodeRoomPersonalSpace         = "room_personal_space"
	CodeRoomMemberNotFound        = "member_not_found"
	CodeRoomMemberChanged         = "room_member_changed"
	CodeIdempotencyKeyReused      = "idempotency_key_reused"
	CodeMemberNotInOrganization   = "member_not_in_organization"
	CodeInvalidRequest            = "invalid_request"
	CodeSpaceAuthorityUnavailable = "space_authority_unavailable"
	CodeRoomNotJoinable           = "room_not_joinable"
)

// RoleRank returns the rank of a role, or 0 for anything that is not one of
// the four closed roles.
func RoleRank(role string) int {
	switch role {
	case RoleViewer:
		return 1
	case RoleEditor:
		return 2
	case RoleManager:
		return 3
	case RoleOwner:
		return 4
	}
	return 0
}

// ValidRole reports whether role is one of the four closed roles.
func ValidRole(role string) bool { return RoleRank(role) > 0 }

// IsElevatedRole reports whether role is manager or owner.
func IsElevatedRole(role string) bool { return RoleRank(role) >= RoleRank(RoleManager) }

// RoleDenial is a refusal by the role matrix. Status is the HTTP status the
// route answers with.
type RoleDenial struct {
	Code    string
	Status  int
	Message string
	Details map[string]any
}

func (d *RoleDenial) Error() string { return d.Code }

func roleForbidden() *RoleDenial {
	return &RoleDenial{Code: CodeRoomRoleForbidden, Status: 403, Message: "your role in this room does not allow that"}
}

func elevatedNeedsOwner() *RoleDenial {
	return &RoleDenial{Code: CodeRoomElevatedNeedsOwner, Status: 403, Message: "only an owner can grant, change or remove a manager or owner"}
}

func soleOwner() *RoleDenial {
	return &RoleDenial{Code: CodeRoomSoleOwner, Status: 409, Message: "the room needs at least one owner"}
}

// CanAdd: rank(actor) >= manager, and a new elevated role needs an owner.
func CanAdd(actorRole, newRole string) *RoleDenial {
	if RoleRank(actorRole) < RoleRank(RoleManager) {
		return roleForbidden()
	}
	if IsElevatedRole(newRole) && actorRole != RoleOwner {
		return elevatedNeedsOwner()
	}
	return nil
}

// CanChangeRole: a self-demotion (or a no-op on yourself) is always allowed
// and a self-promotion never is; changing someone else needs rank manager or
// above, and neither the target's current role nor the new role may be
// elevated unless the actor is an owner. The last owner cannot be demoted.
func CanChangeRole(actorRole, targetCurrent, newRole string, targetIsActor bool, activeOwners int) *RoleDenial {
	if targetIsActor {
		if RoleRank(newRole) > RoleRank(targetCurrent) {
			if IsElevatedRole(newRole) {
				return elevatedNeedsOwner()
			}
			return roleForbidden()
		}
		if targetCurrent == RoleOwner && newRole != RoleOwner && activeOwners <= 1 {
			return soleOwner()
		}
		return nil
	}
	if RoleRank(actorRole) < RoleRank(RoleManager) {
		return roleForbidden()
	}
	if (IsElevatedRole(targetCurrent) || IsElevatedRole(newRole)) && actorRole != RoleOwner {
		return elevatedNeedsOwner()
	}
	if targetCurrent == RoleOwner && newRole != RoleOwner && activeOwners <= 1 {
		return soleOwner()
	}
	return nil
}

// CanRemove: leaving (target is the actor) is always allowed except for the
// last owner; removing someone else needs manager or above and an owner for
// an elevated target.
func CanRemove(actorRole, targetRole string, targetIsActor bool, activeOwners int) *RoleDenial {
	if !targetIsActor {
		if RoleRank(actorRole) < RoleRank(RoleManager) {
			return roleForbidden()
		}
		if IsElevatedRole(targetRole) && actorRole != RoleOwner {
			return elevatedNeedsOwner()
		}
	}
	if targetRole == RoleOwner && activeOwners <= 1 {
		return soleOwner()
	}
	return nil
}

// CanJoin: a self-join role is viewer or editor, never elevated (the table
// CHECK enforces it again).
func CanJoin(joinRole string) *RoleDenial {
	if joinRole == RoleViewer || joinRole == RoleEditor {
		return nil
	}
	return &RoleDenial{Code: CodeRoomNotJoinable, Status: 409, Message: "the room cannot be joined with that role"}
}

// CanManageRoom covers archive, settings and details: rank(actor) >= manager.
func CanManageRoom(actorRole string) *RoleDenial {
	if RoleRank(actorRole) < RoleRank(RoleManager) {
		return roleForbidden()
	}
	return nil
}
