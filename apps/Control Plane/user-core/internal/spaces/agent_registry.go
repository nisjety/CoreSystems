package spaces

import (
	"fmt"
	"regexp"
	"strings"
)

// WS17-S4: the personal agent registry, its access list, add policy and per-room
// admission (ADR 4, SD-04 option A). Control owns the link from an agent to its
// owner, the access list, the add policy and each admission, so a room can only
// admit an agent under authority Control derived itself. Ids, states, revisions
// and reason codes only: no agent name, instructions or content reach this file.
//
// The pure functions here (AdmitDenial, ValidateAgentPolicy, ParseAgentAdmissionMode)
// carry the decisions. agent_repository.go and agent_sweeper.go do the I/O.

// Agent visibility. An owner-visible agent is personal: listed to its owner and to
// its access list only. An org-visible agent is listed to every active org member.
const (
	AgentVisibilityOwner = "owner"
	AgentVisibilityOrg   = "org"
)

// Add policy: who besides the room's owner or manager may add the agent.
const (
	AgentAddPolicyOwnerOnly = "owner_only"
	AgentAddPolicyNobody    = "nobody"
	AgentAddPolicyAnyone    = "anyone"
)

// Lifecycle of an agent definition. Only an active agent may be admitted.
const (
	AgentLifecycleActive    = "active"
	AgentLifecycleSuspended = "suspended"
	AgentLifecycleRetired   = "retired"
)

// Access-list roles on an agent. These are not room roles.
const (
	AgentAccessManager      = "manager"
	AgentAccessCollaborator = "collaborator"
)

// Admission states of one agent in one room. Only an admitted agent may sit on
// the room roster.
const (
	AdmissionStateAdmitted  = "admitted"
	AdmissionStateWithdrawn = "withdrawn"
	AdmissionStateSuspended = "suspended"
)

// Events of the append-only admission ledger (migration 045).
const (
	AdmissionEventAdmitted  = "admitted"
	AdmissionEventWithdrawn = "withdrawn"
	AdmissionEventSuspended = "suspended"
	AdmissionEventWouldSkip = "would_skip"
)

// AdmitDenial reasons, in the order AdmitDenial checks them (WS17 D3).
const (
	AdmitReasonSpaceNotActive       = "space_not_active"
	AdmitReasonPersonalSpace        = "personal_space_not_supported"
	AdmitReasonRoomRoleInsufficient = "room_role_insufficient"
	AdmitReasonAgentSuspended       = "agent_suspended"
	AdmitReasonAgentNotVisible      = "agent_not_visible"
	AdmitReasonAddPolicyNobody      = "add_policy_nobody"
	AdmitReasonAddPolicyOwnerOnly   = "add_policy_owner_only"
	AdmitReasonNotEntitled          = "personal_agents_not_entitled"
	AdmitReasonOwnerNotInRoom       = "owner_not_in_room"
)

// Typed error codes of the agent registry, answered in the typed envelope.
const (
	CodeAgentNotFound            = "agent_not_found"
	CodeAgentSubjectConflict     = "agent_subject_conflict"
	CodeAgentPolicyInvalid       = "agent_policy_invalid"
	CodeAgentAccessForbidden     = "agent_access_forbidden"
	CodeAgentStateChanged        = "agent_state_changed"
	CodePrincipalNotInOrg        = "principal_not_in_org"
	CodeAgentNotAddable          = "agent_not_addable"
	CodeAgentWithdrawForbidden   = "agent_withdraw_forbidden"
	CodeAgentDelegationRequired  = "agent_delegation_required"
	CodeAgentRegistryUnavailable = "agent_registry_unavailable"
)

// AgentSubject is one agent definition as Control holds it. OwnerUserID is ""
// only for a legacy, org-owned definition that no owner has claimed.
type AgentSubject struct {
	OrgID          string
	AgentSubjectID string
	OwnerUserID    string
	Visibility     string
	AddPolicy      string
	Lifecycle      string
	Revision       int64
}

// AgentAdmitFacts is everything AdmitDenial decides on. The caller reads each field
// from Control's own rows inside the same transaction. None of them comes from a
// request body.
type AgentAdmitFacts struct {
	// SpaceActive is true when the room's registration_state is active.
	SpaceActive bool
	SpaceKind   Kind
	// ActorRoomRole is the actor's active role in the room ("" when not a member).
	ActorRoomRole string
	Agent         AgentSubject
	// ActorIsOwner is true when the actor is the agent's owner.
	ActorIsOwner bool
	// ActorAccess is the actor's access-list role on the agent: "", manager or
	// collaborator.
	ActorAccess string
	// OwnerActiveInRoom is true when the agent's owner is an active room member and
	// an active organization member.
	OwnerActiveInRoom bool
	// OrgPersonalAdmit is the organization's personal_agent_admit_entitled bit.
	OrgPersonalAdmit bool
}

// AdmitDenial returns "" when the actor may add the agent to the room, and otherwise
// the first failing reason in the WS17 D3 order. It is pure: every input is a fact
// the caller read from Control's rows.
func AdmitDenial(f AgentAdmitFacts) string {
	if !f.SpaceActive {
		return AdmitReasonSpaceNotActive
	}
	if f.SpaceKind == KindPersonal {
		return AdmitReasonPersonalSpace
	}
	if CanManageRoom(f.ActorRoomRole) != nil {
		return AdmitReasonRoomRoleInsufficient
	}
	if f.Agent.Lifecycle != AgentLifecycleActive {
		return AdmitReasonAgentSuspended
	}
	listed := f.ActorAccess == AgentAccessManager || f.ActorAccess == AgentAccessCollaborator
	if f.Agent.Visibility == AgentVisibilityOwner && !f.ActorIsOwner && !listed {
		return AdmitReasonAgentNotVisible
	}
	switch f.Agent.AddPolicy {
	case AgentAddPolicyNobody:
		return AdmitReasonAddPolicyNobody
	case AgentAddPolicyOwnerOnly:
		if !f.ActorIsOwner && !listed {
			return AdmitReasonAddPolicyOwnerOnly
		}
	}
	if f.Agent.Visibility == AgentVisibilityOwner {
		if !f.OrgPersonalAdmit {
			return AdmitReasonNotEntitled
		}
		if !f.OwnerActiveInRoom {
			return AdmitReasonOwnerNotInRoom
		}
	}
	return ""
}

// CanEditAgentAccess reports whether the actor may change an agent's visibility, add
// policy and access list: its owner, or a manager on its access list.
func CanEditAgentAccess(actorIsOwner bool, actorAccess string) bool {
	return actorIsOwner || actorAccess == AgentAccessManager
}

// CanWithdrawAgent reports whether the actor may withdraw an agent from a room: a
// room owner or manager, or the agent's owner or a manager on its access list. So an
// owner can always take a personal agent out of a room.
func CanWithdrawAgent(actorRoomRole string, actorIsOwner bool, actorAccess string) bool {
	return CanManageRoom(actorRoomRole) == nil || actorIsOwner || actorAccess == AgentAccessManager
}

var agentSubjectIDPattern = regexp.MustCompile(`^agent-[A-Za-z0-9_-]{6,64}$`)

// ValidAgentSubjectID reports whether id has the agent subject shape. It matches the
// CHECK constraint in migration 045.
func ValidAgentSubjectID(id string) bool {
	return agentSubjectIDPattern.MatchString(id)
}

// IsAgentSubjectID reports whether a roster subject is gated by admission. Only
// agent-* service subjects are gated. Every other service identity is unchanged.
func IsAgentSubjectID(subjectID string) bool {
	return strings.HasPrefix(subjectID, "agent-")
}

// ValidAgentAccessRole reports whether role may appear on an access-list entry.
func ValidAgentAccessRole(role string) bool {
	return role == AgentAccessManager || role == AgentAccessCollaborator
}

// ValidateAgentPolicy checks a visibility and add policy pair. An agent may be added
// by anyone in the org only when it is org-visible.
func ValidateAgentPolicy(visibility, addPolicy string) error {
	if visibility != AgentVisibilityOwner && visibility != AgentVisibilityOrg {
		return AgentError(422, CodeAgentPolicyInvalid, "visibility must be owner or org",
			map[string]any{"field": "visibility"})
	}
	switch addPolicy {
	case AgentAddPolicyOwnerOnly, AgentAddPolicyNobody:
	case AgentAddPolicyAnyone:
		if visibility != AgentVisibilityOrg {
			return AgentError(422, CodeAgentPolicyInvalid, "add_policy anyone needs org visibility",
				map[string]any{"field": "add_policy"})
		}
	default:
		return AgentError(422, CodeAgentPolicyInvalid, "add_policy must be owner_only, nobody or anyone",
			map[string]any{"field": "add_policy"})
	}
	return nil
}

// AgentError builds a typed refusal of the agent registry: HTTP status, stable code
// and details. The route layer writes it in the typed error envelope.
func AgentError(status int, code, message string, details map[string]any) *RoomError {
	return roomError(status, code, message, details)
}

// AgentAdmissionMode is SPACE_AGENT_ADMISSION. The zero value and "off" leave the
// roster unchanged and run no owner sweep. observe leaves the roster unchanged but
// writes each declared agent the admission rows do not admit to the ledger as
// would_skip. enforce leaves such an agent off the roster and reports it in
// skipped_unadmitted. This is a rollout control, never an authorization input.
type AgentAdmissionMode string

const (
	AgentAdmissionOff     AgentAdmissionMode = "off"
	AgentAdmissionObserve AgentAdmissionMode = "observe"
	AgentAdmissionEnforce AgentAdmissionMode = "enforce"
)

// ParseAgentAdmissionMode reads SPACE_AGENT_ADMISSION. Unset or blank is off. Any
// other value must be exactly observe or enforce, or the service refuses to start:
// a typo must not silently run a different mode.
func ParseAgentAdmissionMode(raw string) (AgentAdmissionMode, error) {
	switch strings.TrimSpace(raw) {
	case "", "off":
		return AgentAdmissionOff, nil
	case "observe":
		return AgentAdmissionObserve, nil
	case "enforce":
		return AgentAdmissionEnforce, nil
	}
	return AgentAdmissionOff, fmt.Errorf("must be off, observe or enforce")
}
