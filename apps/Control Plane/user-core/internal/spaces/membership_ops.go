package spaces

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
)

// Control-native room membership writer (WS04-S4).
//
// Every operation runs in one transaction that locks the registered_spaces row
// FOR UPDATE first (the same serialization point ReplaceMemberships uses, so a
// declarative sync and an actor write never interleave), re-reads the actor's
// and the target's rows, applies the pure role matrix in roles.go, writes the
// row with source='member_ops', advances the authority and membership
// revisions, appends one ledger row, and commits. The acting user and the
// organization come from the verified delegation at the HTTP layer; nothing
// here trusts a body field for either. Rooms only: personal Spaces and the
// organization room (roster_mode='org_derived') are refused, and so is any
// subject that is not a human organization member (agents stay Control
// service subjects bound by the declarative path).
//
// The ledger holds ids, roles and counts only (no names, no content), so a
// Zero Data Retention organization behaves identically and no model is called.

// Membership row sources. A Control actor's write takes ownership of the row,
// so the declarative Application sync can no longer re-role, reactivate or
// deactivate it.
const (
	SourceRegistration    = "registration"
	SourceApplicationSync = "application_sync"
	SourceMemberOps       = "member_ops"
	SourceSelfJoin        = "self_join"
)

// Ledger actions and actor kinds (see migration 034).
const (
	ActionMemberAdded   = "member_added"
	ActionMemberJoined  = "member_joined"
	ActionRoleChanged   = "role_changed"
	ActionMemberRemoved = "member_removed"
	ActionMemberLeft    = "member_left"

	ActorKindUser    = "user"
	ActorKindService = "service"

	// The ledger actor of a declarative replace: the Application lifecycle
	// workload that is allowed to call PUT .../memberships.
	applicationLifecycleActor = "application-space-lifecycle"
)

// RoomError is a typed refusal of the room writer. Status is the HTTP status;
// Code is the stable machine-readable code of the typed error envelope.
type RoomError struct {
	Status  int
	Code    string
	Message string
	Details map[string]any
}

func (e *RoomError) Error() string { return e.Code }

func roomError(status int, code, message string, details map[string]any) *RoomError {
	return &RoomError{Status: status, Code: code, Message: message, Details: details}
}

func roomNotFound() *RoomError {
	// One body for an unknown Space, another organization's Space and a Space
	// the caller is not a member of: nothing here may reveal which it was.
	return roomError(404, CodeRoomNotFound, "room not found", nil)
}

func invalidRequest(field, message string) *RoomError {
	return roomError(400, CodeInvalidRequest, message, map[string]any{"field": field})
}

func denialError(d *RoleDenial) *RoomError {
	return &RoomError{Status: d.Status, Code: d.Code, Message: d.Message, Details: d.Details}
}

// MembershipChangeEvent is the current-fact statement published after commit
// on controlplane.space.membership_changed. ids, role and counts only.
type MembershipChangeEvent struct {
	EventID   string
	SpaceRef  string
	OrgID     string
	SubjectID string
	Action    string
	Role      string
	ActorKind string
	Active    bool
	SpaceSeq  int64
}

// ReplaceMembershipsDetail is the full outcome of a declarative replace.
type ReplaceMembershipsDetail struct {
	Revisions               *AuthorityRevision
	OrgID                   string
	RevokedUserSubjects     []string
	ReactivatedUserSubjects []string
	// SkippedControlOwned counts declared members left untouched because their
	// row is owned by a Control actor (source member_ops or self_join).
	SkippedControlOwned int
	// Events lists the user rows that became active or inactive, with the
	// ledger event that recorded each (role-only changes are ledger-only).
	Events []MembershipChangeEvent
}

type ledgerEntry struct {
	SpaceRef       string
	OrgID          string
	SpaceSeq       int64
	Action         string
	ActorKind      string
	ActorID        string
	SubjectType    string
	SubjectID      string
	FromRole       string
	ToRole         string
	IdempotencyKey string
}

func insertMembershipEvent(ctx context.Context, tx pgx.Tx, e ledgerEntry) (string, error) {
	eventID := "sme_" + uuid.NewString()
	if _, err := tx.Exec(ctx, `
		INSERT INTO space_membership_events
			(event_id, space_ref, org_id, space_seq, action, actor_kind, actor_id,
			 subject_type, subject_id, from_role, to_role, idempotency_key)
		VALUES ($1,$2,$3,$4,$5,$6,$7,NULLIF($8,''),NULLIF($9,''),NULLIF($10,''),NULLIF($11,''),NULLIF($12,''))`,
		eventID, e.SpaceRef, e.OrgID, e.SpaceSeq, e.Action, e.ActorKind, e.ActorID,
		e.SubjectType, e.SubjectID, e.FromRole, e.ToRole, e.IdempotencyKey,
	); err != nil {
		return "", fmt.Errorf("append Space membership ledger row: %w", err)
	}
	return eventID, nil
}

// MemberOpResult is the outcome of add, change-role and remove.
type MemberOpResult struct {
	SpaceRef           string
	SubjectID          string
	Role               string
	FromRole           string
	Changed            bool
	Replayed           bool
	Left               bool
	MembershipRevision int64
	AuthorityRevision  int64
	MemberCount        int
	// Event is set when the operation changed a row. Publish it only when
	// Action is not role_changed (role-only changes are ledger-only).
	Event *MembershipChangeEvent
}

// MemberOpRequest carries one writer call. ActorID and OrgID come from the
// signed delegation.
type MemberOpRequest struct {
	SpaceRef         string
	OrgID            string
	ActorID          string
	SubjectID        string
	Role             string
	IdempotencyKey   string
	ExpectedRevision *int64
}

type roomOp struct {
	tx         pgx.Tx
	space      *RegisteredSpace
	actorID    string
	actorRole  string
	rosterMode string
	archived   bool
	revisions  AuthorityRevision
}

func (r *Repository) roomOpPrelude(ctx context.Context, tx pgx.Tx, req MemberOpRequest) (*roomOp, error) {
	space, err := registeredSpaceForUpdate(ctx, tx, req.SpaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("lock Space for membership operation: %w", err)
	}
	if space.OrgID != req.OrgID {
		return nil, roomNotFound()
	}
	op := &roomOp{tx: tx, space: space, actorID: req.ActorID}
	// The actor must be an active member of the room and an active member of
	// the organization with an active user; anything else is indistinguishable
	// from an unknown room.
	err = tx.QueryRow(ctx, `
		SELECT m.role FROM space_memberships m
		WHERE m.space_ref=$1 AND m.subject_type='user' AND m.subject_id=$2 AND m.active=TRUE
		  AND EXISTS (
			SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
			WHERE u.user_id=$2 AND u.org_id=$3 AND u.status='active' AND x.status='active'
		  )
		FOR UPDATE OF m`, space.SpaceRef, req.ActorID, req.OrgID).Scan(&op.actorRole)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("read acting Space member: %w", err)
	}
	switch space.RegistrationState {
	case "active":
	case "archived":
		op.archived = true
	default:
		return nil, roomNotFound()
	}
	if space.Kind == KindPersonal {
		return nil, roomError(409, CodeRoomPersonalSpace, "a personal Space has no members to manage", nil)
	}
	if err := tx.QueryRow(ctx, `SELECT roster_mode FROM registered_spaces WHERE space_ref=$1`, space.SpaceRef).Scan(&op.rosterMode); err != nil {
		return nil, fmt.Errorf("read Space roster mode: %w", err)
	}
	if err := tx.QueryRow(ctx, `
		SELECT authority_revision, membership_revision, privacy_revision,
		       recipient_audience_revision, entitlement_revision
		FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, space.SpaceRef).Scan(
		&op.revisions.Authority, &op.revisions.Membership, &op.revisions.Privacy,
		&op.revisions.RecipientAudience, &op.revisions.Entitlement,
	); err != nil {
		return nil, fmt.Errorf("lock Space membership revision: %w", err)
	}
	return op, nil
}

func (op *roomOp) refuseDerivedOrArchived() error {
	if op.rosterMode == RosterModeOrgDerived {
		return roomError(409, CodeRoomRosterDerived, "this room's members follow the organization", nil)
	}
	if op.archived {
		return roomError(409, CodeRoomArchived, "this room is archived", nil)
	}
	return nil
}

func (op *roomOp) activeOwners(ctx context.Context) (int, error) {
	var owners int
	err := op.tx.QueryRow(ctx, `
		SELECT count(*) FROM space_memberships m
		WHERE m.space_ref=$1 AND m.subject_type='user' AND m.role='owner' AND m.active=TRUE
		  AND EXISTS (
			SELECT 1 FROM user_org_memberships u
			WHERE u.user_id=m.subject_id AND u.org_id=$2 AND u.status='active'
		  )`, op.space.SpaceRef, op.space.OrgID).Scan(&owners)
	if err != nil {
		return 0, fmt.Errorf("count active Space owners: %w", err)
	}
	return owners, nil
}

func (op *roomOp) memberCount(ctx context.Context) (int, error) {
	var count int
	err := op.tx.QueryRow(ctx, `
		SELECT count(*) FROM space_memberships
		WHERE space_ref=$1 AND subject_type='user' AND active=TRUE`, op.space.SpaceRef).Scan(&count)
	if err != nil {
		return 0, fmt.Errorf("count Space members: %w", err)
	}
	return count, nil
}

// result builds a no-change (or replay) result at the current revisions.
func (op *roomOp) result(ctx context.Context, req MemberOpRequest, role string) (*MemberOpResult, error) {
	count, err := op.memberCount(ctx)
	if err != nil {
		return nil, err
	}
	return &MemberOpResult{
		SpaceRef: op.space.SpaceRef, SubjectID: req.SubjectID, Role: role,
		MembershipRevision: op.revisions.Membership, AuthorityRevision: op.revisions.Authority,
		MemberCount: count,
	}, nil
}

// replay returns the stored ledger fact for an idempotency key, nil when the
// key has not been used on this Space.
func (op *roomOp) replay(ctx context.Context, key string) (*ledgerEntry, error) {
	if key == "" {
		return nil, nil
	}
	var e ledgerEntry
	var subjectID, fromRole, toRole sql.NullString
	err := op.tx.QueryRow(ctx, `
		SELECT action, actor_id, subject_id, from_role, to_role FROM space_membership_events
		WHERE space_ref=$1 AND idempotency_key=$2`, op.space.SpaceRef, key).Scan(&e.Action, &e.ActorID, &subjectID, &fromRole, &toRole)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read Space membership idempotency key: %w", err)
	}
	e.SubjectID, e.FromRole, e.ToRole = subjectID.String, fromRole.String, toRole.String
	return &e, nil
}

// commitChange advances the revisions, appends the ledger row and commits.
func (op *roomOp) commitChange(ctx context.Context, req MemberOpRequest, entry ledgerEntry, result *MemberOpResult) (*MemberOpResult, error) {
	var next AuthorityRevision
	if err := op.tx.QueryRow(ctx, `
		UPDATE space_authority_revisions
		SET authority_revision=authority_revision+1,
		    membership_revision=membership_revision+1,
		    updated_at=NOW()
		WHERE space_ref=$1
		RETURNING authority_revision, membership_revision, privacy_revision,
		          recipient_audience_revision, entitlement_revision`, op.space.SpaceRef).Scan(
		&next.Authority, &next.Membership, &next.Privacy, &next.RecipientAudience, &next.Entitlement,
	); err != nil {
		return nil, fmt.Errorf("advance Space membership revision: %w", err)
	}
	op.revisions = next
	entry.SpaceRef, entry.OrgID, entry.SpaceSeq = op.space.SpaceRef, op.space.OrgID, next.Authority
	entry.ActorKind, entry.ActorID, entry.IdempotencyKey = ActorKindUser, op.actorID, req.IdempotencyKey
	entry.SubjectType, entry.SubjectID = "user", req.SubjectID
	eventID, err := insertMembershipEvent(ctx, op.tx, entry)
	if err != nil {
		return nil, err
	}
	count, err := op.memberCount(ctx)
	if err != nil {
		return nil, err
	}
	result.Changed = true
	result.MembershipRevision, result.AuthorityRevision, result.MemberCount = next.Membership, next.Authority, count
	result.Event = &MembershipChangeEvent{
		EventID: eventID, SpaceRef: op.space.SpaceRef, OrgID: op.space.OrgID, SubjectID: req.SubjectID,
		Action: entry.Action, Role: entry.ToRole, ActorKind: ActorKindUser, SpaceSeq: next.Authority,
		Active: entry.Action == ActionMemberAdded || entry.Action == ActionMemberJoined,
	}
	if err := op.tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit Space membership operation: %w", err)
	}
	return result, nil
}

func normalizeMemberRequest(req *MemberOpRequest) error {
	req.SpaceRef = strings.TrimSpace(req.SpaceRef)
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.SubjectID = strings.TrimSpace(req.SubjectID)
	req.Role = strings.TrimSpace(req.Role)
	req.IdempotencyKey = strings.TrimSpace(req.IdempotencyKey)
	if req.SpaceRef == "" {
		return invalidRequest("space_ref", "space_ref is required")
	}
	if req.OrgID == "" || req.ActorID == "" {
		// Defence in depth: the HTTP layer already requires a verified delegation.
		return roomNotFound()
	}
	if req.SubjectID == "" {
		return invalidRequest("subject_id", "subject_id is required")
	}
	if len(req.IdempotencyKey) > 128 {
		return invalidRequest("idempotency_key", "idempotency_key is too long")
	}
	return nil
}

func (r *Repository) beginRoomOp(ctx context.Context, req MemberOpRequest) (*roomOp, error) {
	if r == nil || r.db == nil {
		return nil, roomError(503, CodeSpaceAuthorityUnavailable, "space authority is unavailable", nil)
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin Space membership operation: %w", err)
	}
	op, err := r.roomOpPrelude(ctx, tx, req)
	if err != nil {
		_ = tx.Rollback(ctx)
		return nil, err
	}
	return op, nil
}

// AddMember sets a human organization member active in a managed room with
// the given role (default editor). Set-to-state: an already-active target
// returns changed=false and keeps its role (a role change is ChangeMemberRole).
func (r *Repository) AddMember(ctx context.Context, req MemberOpRequest) (*MemberOpResult, error) {
	if err := normalizeMemberRequest(&req); err != nil {
		return nil, err
	}
	if req.Role == "" {
		req.Role = RoleEditor
	}
	if !ValidRole(req.Role) {
		return nil, invalidRequest("role", "role must be viewer, editor, manager or owner")
	}
	op, err := r.beginRoomOp(ctx, req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = op.tx.Rollback(ctx) }()
	if err := op.refuseDerivedOrArchived(); err != nil {
		return nil, err
	}
	if denial := CanAdd(op.actorRole, req.Role); denial != nil {
		return nil, denialError(denial)
	}
	if prior, err := op.replay(ctx, req.IdempotencyKey); err != nil {
		return nil, err
	} else if prior != nil {
		if prior.Action != ActionMemberAdded || prior.SubjectID != req.SubjectID || prior.ToRole != req.Role || prior.ActorID != req.ActorID {
			return nil, roomError(409, CodeIdempotencyKeyReused, "idempotency_key was used for a different request", nil)
		}
		result, err := op.result(ctx, req, req.Role)
		if err != nil {
			return nil, err
		}
		result.Replayed = true
		return result, nil
	}
	var wasActive bool
	var currentRole string
	err = op.tx.QueryRow(ctx, `
		SELECT active, role FROM space_memberships
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2 FOR UPDATE`,
		op.space.SpaceRef, req.SubjectID).Scan(&wasActive, &currentRole)
	if err != nil && !errors.Is(err, pgx.ErrNoRows) {
		return nil, fmt.Errorf("read target Space member: %w", err)
	}
	if err == nil && wasActive {
		return op.result(ctx, req, currentRole)
	}
	var eligible bool
	if err := op.tx.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
			WHERE u.user_id=$1 AND u.org_id=$2 AND u.status='active' AND x.status='active'
		)`, req.SubjectID, op.space.OrgID).Scan(&eligible); err != nil {
		return nil, fmt.Errorf("verify target organization membership: %w", err)
	}
	if !eligible {
		return nil, roomError(422, CodeMemberNotInOrganization, "that person is not an active member of the organization", nil)
	}
	if _, err := op.tx.Exec(ctx, `
		INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, active, granted_by, source)
		VALUES ($1,'user',$2,$3,TRUE,$4,'member_ops')
		ON CONFLICT (space_ref, subject_type, subject_id) DO UPDATE
		SET role=EXCLUDED.role, active=TRUE, revision=space_memberships.revision+1,
		    granted_by=EXCLUDED.granted_by, source='member_ops', updated_at=NOW()`,
		op.space.SpaceRef, req.SubjectID, req.Role, req.ActorID); err != nil {
		return nil, fmt.Errorf("add Space member: %w", err)
	}
	return op.commitChange(ctx, req, ledgerEntry{Action: ActionMemberAdded, ToRole: req.Role},
		&MemberOpResult{SpaceRef: op.space.SpaceRef, SubjectID: req.SubjectID, Role: req.Role})
}

// ChangeMemberRole sets an active human member's role. Set-to-state; the
// optional ExpectedRevision is the row revision the caller's roster showed.
func (r *Repository) ChangeMemberRole(ctx context.Context, req MemberOpRequest) (*MemberOpResult, error) {
	if err := normalizeMemberRequest(&req); err != nil {
		return nil, err
	}
	if !ValidRole(req.Role) {
		return nil, invalidRequest("role", "role must be viewer, editor, manager or owner")
	}
	op, err := r.beginRoomOp(ctx, req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = op.tx.Rollback(ctx) }()
	if err := op.refuseDerivedOrArchived(); err != nil {
		return nil, err
	}
	var targetRole string
	var targetRevision int64
	err = op.tx.QueryRow(ctx, `
		SELECT role, revision FROM space_memberships
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2 AND active=TRUE FOR UPDATE`,
		op.space.SpaceRef, req.SubjectID).Scan(&targetRole, &targetRevision)
	if errors.Is(err, pgx.ErrNoRows) {
		if denial := CanManageRoom(op.actorRole); denial != nil && req.SubjectID != req.ActorID {
			return nil, denialError(denial)
		}
		return nil, roomError(404, CodeRoomMemberNotFound, "member not found", nil)
	}
	if err != nil {
		return nil, fmt.Errorf("read target Space member: %w", err)
	}
	isActor := req.SubjectID == req.ActorID
	owners, err := op.activeOwners(ctx)
	if err != nil {
		return nil, err
	}
	if denial := CanChangeRole(op.actorRole, targetRole, req.Role, isActor, owners); denial != nil {
		return nil, denialError(denial)
	}
	if req.ExpectedRevision != nil && *req.ExpectedRevision != targetRevision {
		return nil, roomError(409, CodeRoomMemberChanged, "the member changed since you loaded the roster",
			map[string]any{"current_revision": targetRevision})
	}
	if prior, err := op.replay(ctx, req.IdempotencyKey); err != nil {
		return nil, err
	} else if prior != nil {
		if prior.Action != ActionRoleChanged || prior.SubjectID != req.SubjectID || prior.ToRole != req.Role || prior.ActorID != req.ActorID {
			return nil, roomError(409, CodeIdempotencyKeyReused, "idempotency_key was used for a different request", nil)
		}
		result, err := op.result(ctx, req, req.Role)
		if err != nil {
			return nil, err
		}
		result.Replayed, result.FromRole = true, prior.FromRole
		return result, nil
	}
	if targetRole == req.Role {
		result, err := op.result(ctx, req, req.Role)
		if err != nil {
			return nil, err
		}
		result.FromRole = targetRole
		return result, nil
	}
	if _, err := op.tx.Exec(ctx, `
		UPDATE space_memberships
		SET role=$3, revision=revision+1, granted_by=$4, source='member_ops', updated_at=NOW()
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2`,
		op.space.SpaceRef, req.SubjectID, req.Role, req.ActorID); err != nil {
		return nil, fmt.Errorf("change Space member role: %w", err)
	}
	return op.commitChange(ctx, req, ledgerEntry{Action: ActionRoleChanged, FromRole: targetRole, ToRole: req.Role},
		&MemberOpResult{SpaceRef: op.space.SpaceRef, SubjectID: req.SubjectID, Role: req.Role, FromRole: targetRole})
}

// RemoveMember deactivates a human member (a leave when the subject is the
// actor). The organization room refuses; an archived room allows leave only.
func (r *Repository) RemoveMember(ctx context.Context, req MemberOpRequest) (*MemberOpResult, error) {
	if err := normalizeMemberRequest(&req); err != nil {
		return nil, err
	}
	op, err := r.beginRoomOp(ctx, req)
	if err != nil {
		return nil, err
	}
	defer func() { _ = op.tx.Rollback(ctx) }()
	isActor := req.SubjectID == req.ActorID
	if op.rosterMode == RosterModeOrgDerived {
		return nil, roomError(409, CodeRoomRosterDerived, "this room's members follow the organization", nil)
	}
	if op.archived && !isActor {
		return nil, roomError(409, CodeRoomArchived, "this room is archived", nil)
	}
	if !isActor {
		if denial := CanManageRoom(op.actorRole); denial != nil {
			return nil, denialError(denial)
		}
	}
	var targetRole string
	err = op.tx.QueryRow(ctx, `
		SELECT role FROM space_memberships
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2 AND active=TRUE FOR UPDATE`,
		op.space.SpaceRef, req.SubjectID).Scan(&targetRole)
	if errors.Is(err, pgx.ErrNoRows) {
		result, err := op.result(ctx, req, "")
		if err != nil {
			return nil, err
		}
		return result, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read target Space member: %w", err)
	}
	owners, err := op.activeOwners(ctx)
	if err != nil {
		return nil, err
	}
	if denial := CanRemove(op.actorRole, targetRole, isActor, owners); denial != nil {
		return nil, denialError(denial)
	}
	action := ActionMemberRemoved
	if isActor {
		action = ActionMemberLeft
	}
	if prior, err := op.replay(ctx, req.IdempotencyKey); err != nil {
		return nil, err
	} else if prior != nil {
		if prior.Action != action || prior.SubjectID != req.SubjectID || prior.ActorID != req.ActorID {
			return nil, roomError(409, CodeIdempotencyKeyReused, "idempotency_key was used for a different request", nil)
		}
		result, err := op.result(ctx, req, "")
		if err != nil {
			return nil, err
		}
		result.Replayed, result.Left = true, isActor
		return result, nil
	}
	if _, err := op.tx.Exec(ctx, `
		UPDATE space_memberships
		SET active=FALSE, revision=revision+1, granted_by=$3, source='member_ops', updated_at=NOW()
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2`,
		op.space.SpaceRef, req.SubjectID, req.ActorID); err != nil {
		return nil, fmt.Errorf("remove Space member: %w", err)
	}
	result, err := op.commitChange(ctx, req, ledgerEntry{Action: action, FromRole: targetRole},
		&MemberOpResult{SpaceRef: op.space.SpaceRef, SubjectID: req.SubjectID, FromRole: targetRole})
	if err != nil {
		return nil, err
	}
	result.Left = isActor
	return result, nil
}

// AudienceCandidate is one person the Space's recipient audience should hold.
type AudienceCandidate struct {
	SubjectID string `json:"subject_id"`
	Role      string `json:"role"`
}

// AudienceCandidates is the read Application's audience reconcile consumes:
// the active human members that are also active organization members, with the
// revisions to compare against. It carries ids and roles only.
type AudienceCandidates struct {
	SpaceRef                  string              `json:"space_ref"`
	OrgID                     string              `json:"org_id"`
	State                     string              `json:"state"`
	MembershipRevision        int64               `json:"membership_revision"`
	RecipientAudienceRevision int64               `json:"recipient_audience_revision"`
	Members                   []AudienceCandidate `json:"members"`
}

// AudienceCandidates reads one consistent snapshot. Unknown Space gives
// room_not_found, a personal Space room_personal_space.
func (r *Repository) AudienceCandidates(ctx context.Context, spaceRef string) (*AudienceCandidates, error) {
	spaceRef = strings.TrimSpace(spaceRef)
	if spaceRef == "" {
		return nil, invalidRequest("space_ref", "space_ref is required")
	}
	if r == nil || r.db == nil {
		return nil, roomError(503, CodeSpaceAuthorityUnavailable, "space authority is unavailable", nil)
	}
	tx, err := r.db.Pool.BeginTx(ctx, pgx.TxOptions{IsoLevel: pgx.RepeatableRead, AccessMode: pgx.ReadOnly})
	if err != nil {
		return nil, fmt.Errorf("begin audience candidates read: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	out := &AudienceCandidates{SpaceRef: spaceRef, Members: []AudienceCandidate{}}
	var kind Kind
	err = tx.QueryRow(ctx, `
		SELECT s.org_id, s.space_kind, s.registration_state, v.membership_revision, v.recipient_audience_revision
		FROM registered_spaces s JOIN space_authority_revisions v ON v.space_ref=s.space_ref
		WHERE s.space_ref=$1`, spaceRef).Scan(&out.OrgID, &kind, &out.State, &out.MembershipRevision, &out.RecipientAudienceRevision)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("read Space for audience candidates: %w", err)
	}
	if kind == KindPersonal {
		return nil, roomError(409, CodeRoomPersonalSpace, "a personal Space has no recipient audience to reconcile", nil)
	}
	rows, err := tx.Query(ctx, `
		SELECT m.subject_id, m.role FROM space_memberships m
		WHERE m.space_ref=$1 AND m.subject_type='user' AND m.active=TRUE
		  AND EXISTS (
			SELECT 1 FROM user_org_memberships u
			WHERE u.user_id=m.subject_id AND u.org_id=$2 AND u.status='active'
		  )
		ORDER BY m.subject_id`, spaceRef, out.OrgID)
	if err != nil {
		return nil, fmt.Errorf("read audience candidates: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var candidate AudienceCandidate
		if err := rows.Scan(&candidate.SubjectID, &candidate.Role); err != nil {
			return nil, fmt.Errorf("scan audience candidate: %w", err)
		}
		out.Members = append(out.Members, candidate)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("read audience candidates: %w", err)
	}
	return out, nil
}
