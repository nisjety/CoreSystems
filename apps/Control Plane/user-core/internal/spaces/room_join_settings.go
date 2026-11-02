package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
)

// Self-join and room settings (WS04-S8). Same discipline as membership_ops.go:
// one transaction, the registered_spaces row locked FOR UPDATE first, actor and
// organization from the signed delegation only, one ledger row per change, ids
// and roles only (no content, no model call, ZDR-neutral).
//
// Self-join writes ONE space_memberships row (source 'self_join'). It never
// writes resource_grants or an effect policy, so joining a room never widens an
// owner-plane ACL: a joined member sees only what the room's own sharing already
// gives its members.

// JoinRequest carries one self-join. ActorID and OrgID come from the delegation.
type JoinRequest struct {
	SpaceRef       string
	OrgID          string
	ActorID        string
	IdempotencyKey string
}

// JoinSpace makes the caller an active member of an open room with the room's
// join role. A room that is private, not a managed room, outside the caller's
// organization, or in an organization whose directory policy is off is
// indistinguishable from an unknown room (404 room_not_found). Set-to-state: an
// already-active member returns changed=false and keeps their role. A member a
// manager removed earlier may rejoin an open room.
func (r *Repository) JoinSpace(ctx context.Context, req JoinRequest) (*MemberOpResult, error) {
	req.SpaceRef = strings.TrimSpace(req.SpaceRef)
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.IdempotencyKey = strings.TrimSpace(req.IdempotencyKey)
	if req.SpaceRef == "" {
		return nil, invalidRequest("space_ref", "space_ref is required")
	}
	if req.OrgID == "" || req.ActorID == "" {
		return nil, roomNotFound()
	}
	if len(req.IdempotencyKey) > 128 {
		return nil, invalidRequest("idempotency_key", "idempotency_key is too long")
	}
	if r == nil || r.db == nil {
		return nil, roomError(503, CodeSpaceAuthorityUnavailable, "space authority is unavailable", nil)
	}
	tx, err := r.db.Pool.Begin(ctx)
	if err != nil {
		return nil, fmt.Errorf("begin Space join: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	space, err := registeredSpaceForUpdate(ctx, tx, req.SpaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("lock Space for join: %w", err)
	}
	if space.OrgID != req.OrgID || space.Kind != KindRoom {
		return nil, roomNotFound()
	}
	var callerActive bool
	if err := tx.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
			WHERE u.user_id=$1 AND u.org_id=$2 AND u.status='active' AND x.status='active'
		)`, req.ActorID, req.OrgID).Scan(&callerActive); err != nil {
		return nil, fmt.Errorf("verify joining organization member: %w", err)
	}
	if !callerActive {
		return nil, roomNotFound()
	}
	var rosterMode, visibility, joinRole string
	if err := tx.QueryRow(ctx, `
		SELECT roster_mode, visibility, join_role FROM registered_spaces WHERE space_ref=$1`,
		space.SpaceRef).Scan(&rosterMode, &visibility, &joinRole); err != nil {
		return nil, fmt.Errorf("read Space join facts: %w", err)
	}
	enabled, err := directoryEnabledTx(ctx, tx, space.OrgID)
	if err != nil {
		return nil, err
	}
	// Private rooms, non-managed rooms and disabled organizations all answer
	// the same 404 as an unknown room.
	if visibility != VisibilityOrgPublic || rosterMode != RosterModeManaged || !enabled {
		return nil, roomNotFound()
	}
	switch space.RegistrationState {
	case "active":
	case RegistrationStateArchived:
		return nil, roomError(409, CodeRoomArchived, "this room is archived", nil)
	default:
		return nil, roomNotFound()
	}
	if denial := CanJoin(joinRole); denial != nil {
		return nil, denialError(denial)
	}

	op := &roomOp{tx: tx, space: space, actorID: req.ActorID, rosterMode: rosterMode}
	if err := tx.QueryRow(ctx, `
		SELECT authority_revision, membership_revision, privacy_revision,
		       recipient_audience_revision, entitlement_revision
		FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, space.SpaceRef).Scan(
		&op.revisions.Authority, &op.revisions.Membership, &op.revisions.Privacy,
		&op.revisions.RecipientAudience, &op.revisions.Entitlement,
	); err != nil {
		return nil, fmt.Errorf("lock Space membership revision: %w", err)
	}
	memberReq := MemberOpRequest{
		SpaceRef: space.SpaceRef, OrgID: space.OrgID, ActorID: req.ActorID,
		SubjectID: req.ActorID, IdempotencyKey: req.IdempotencyKey,
	}

	var wasActive bool
	var currentRole string
	err = tx.QueryRow(ctx, `
		SELECT active, role FROM space_memberships
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2 FOR UPDATE`,
		space.SpaceRef, req.ActorID).Scan(&wasActive, &currentRole)
	if err != nil && !errors.Is(err, pgx.ErrNoRows) {
		return nil, fmt.Errorf("read joining Space member: %w", err)
	}
	hasRow := err == nil

	if prior, err := op.replay(ctx, req.IdempotencyKey); err != nil {
		return nil, err
	} else if prior != nil {
		if prior.Action != ActionMemberJoined || prior.SubjectID != req.ActorID || prior.ActorID != req.ActorID {
			return nil, roomError(409, CodeIdempotencyKeyReused, "idempotency_key was used for a different request", nil)
		}
		role := prior.ToRole
		if hasRow && wasActive {
			role = currentRole
		}
		result, rerr := op.result(ctx, memberReq, role)
		if rerr != nil {
			return nil, rerr
		}
		result.Replayed = true
		return result, nil
	}

	if hasRow && wasActive {
		return op.result(ctx, memberReq, currentRole)
	}

	if _, err := tx.Exec(ctx, `
		INSERT INTO space_memberships (space_ref, subject_type, subject_id, role, active, granted_by, source)
		VALUES ($1,'user',$2,$3,TRUE,$2,'self_join')
		ON CONFLICT (space_ref, subject_type, subject_id) DO UPDATE
		SET role=EXCLUDED.role, active=TRUE, revision=space_memberships.revision+1,
		    granted_by=EXCLUDED.granted_by, source='self_join', updated_at=NOW()`,
		space.SpaceRef, req.ActorID, joinRole); err != nil {
		return nil, fmt.Errorf("join Space: %w", err)
	}
	return op.commitChange(ctx, memberReq, ledgerEntry{Action: ActionMemberJoined, ToRole: joinRole},
		&MemberOpResult{SpaceRef: space.SpaceRef, SubjectID: req.ActorID, Role: joinRole})
}

// Ledger actions of a settings change (see migration 034's action CHECK).
const (
	ActionVisibilityChanged = "visibility_changed"
	ActionSettingsChanged   = "settings_changed"
)

// SettingsRequest carries one settings change. A nil field is left as it is.
// ActorID and OrgID come from the signed delegation.
type SettingsRequest struct {
	SpaceRef       string
	OrgID          string
	ActorID        string
	Visibility     *string
	JoinRole       *string
	IdempotencyKey string
}

// SettingsResult is the set-to-state outcome.
type SettingsResult struct {
	SpaceRef          string
	Visibility        string
	JoinRole          string
	Changed           bool
	Replayed          bool
	AuthorityRevision int64
	PrivacyRevision   int64
}

// UpdateRoomSettings sets a managed room's visibility and join role. Manager or
// owner only. org_public needs the organization's directory policy to be on
// (403 directory_disabled otherwise); the organization room and archived rooms
// refuse. A visibility change advances privacy_revision and authority_revision,
// so a decision issued before it goes stale; a join-role-only change advances
// authority_revision only. Set-to-state: an unchanged request writes nothing.
func (r *Repository) UpdateRoomSettings(ctx context.Context, req SettingsRequest) (*SettingsResult, error) {
	req.SpaceRef = strings.TrimSpace(req.SpaceRef)
	req.OrgID = strings.TrimSpace(req.OrgID)
	req.ActorID = strings.TrimSpace(req.ActorID)
	req.IdempotencyKey = strings.TrimSpace(req.IdempotencyKey)
	if req.SpaceRef == "" {
		return nil, invalidRequest("space_ref", "space_ref is required")
	}
	if req.OrgID == "" || req.ActorID == "" {
		return nil, roomNotFound()
	}
	if req.Visibility == nil && req.JoinRole == nil {
		return nil, invalidRequest("settings", "at least one of visibility or join_role is required")
	}
	if req.Visibility != nil && !ValidVisibility(strings.TrimSpace(*req.Visibility)) {
		return nil, invalidRequest("visibility", "visibility must be private or org_public")
	}
	if req.JoinRole != nil && (*req.JoinRole != RoleViewer && *req.JoinRole != RoleEditor) {
		return nil, invalidRequest("join_role", "join_role must be viewer or editor")
	}
	if len(req.IdempotencyKey) > 128 {
		return nil, invalidRequest("idempotency_key", "idempotency_key is too long")
	}
	op, err := r.beginRoomOp(ctx, MemberOpRequest{SpaceRef: req.SpaceRef, OrgID: req.OrgID, ActorID: req.ActorID})
	if err != nil {
		return nil, err
	}
	defer func() { _ = op.tx.Rollback(ctx) }()

	if denial := CanManageRoom(op.actorRole); denial != nil {
		return nil, denialError(denial)
	}
	if err := op.refuseDerivedOrArchived(); err != nil {
		return nil, err
	}
	var curVisibility, curJoinRole string
	if err := op.tx.QueryRow(ctx, `
		SELECT visibility, join_role FROM registered_spaces WHERE space_ref=$1`,
		op.space.SpaceRef).Scan(&curVisibility, &curJoinRole); err != nil {
		return nil, fmt.Errorf("read Space settings: %w", err)
	}
	newVisibility, newJoinRole := curVisibility, curJoinRole
	if req.Visibility != nil {
		newVisibility = strings.TrimSpace(*req.Visibility)
	}
	if req.JoinRole != nil {
		newJoinRole = *req.JoinRole
	}
	visibilityChanged := newVisibility != curVisibility
	joinRoleChanged := newJoinRole != curJoinRole
	action := ActionSettingsChanged
	if visibilityChanged {
		action = ActionVisibilityChanged
	}

	if req.IdempotencyKey != "" {
		prior, err := op.replay(ctx, req.IdempotencyKey)
		if err != nil {
			return nil, err
		}
		if prior != nil {
			if (prior.Action != ActionVisibilityChanged && prior.Action != ActionSettingsChanged) || prior.ActorID != req.ActorID {
				return nil, roomError(409, CodeIdempotencyKeyReused, "idempotency_key was used for a different request", nil)
			}
			return &SettingsResult{
				SpaceRef: op.space.SpaceRef, Visibility: curVisibility, JoinRole: curJoinRole, Replayed: true,
				AuthorityRevision: op.revisions.Authority, PrivacyRevision: op.revisions.Privacy,
			}, nil
		}
	}
	if !visibilityChanged && !joinRoleChanged {
		return &SettingsResult{
			SpaceRef: op.space.SpaceRef, Visibility: curVisibility, JoinRole: curJoinRole,
			AuthorityRevision: op.revisions.Authority, PrivacyRevision: op.revisions.Privacy,
		}, nil
	}
	if visibilityChanged && newVisibility == VisibilityOrgPublic {
		enabled, err := directoryEnabledTx(ctx, op.tx, op.space.OrgID)
		if err != nil {
			return nil, err
		}
		if !enabled {
			return nil, roomError(403, CodeDirectoryDisabled, "your organization has not turned on the room directory", nil)
		}
	}
	if _, err := op.tx.Exec(ctx, `
		UPDATE registered_spaces SET visibility=$2, join_role=$3, updated_at=NOW() WHERE space_ref=$1`,
		op.space.SpaceRef, newVisibility, newJoinRole); err != nil {
		return nil, fmt.Errorf("update Space settings: %w", err)
	}
	var next AuthorityRevision
	if err := op.tx.QueryRow(ctx, `
		UPDATE space_authority_revisions
		SET authority_revision=authority_revision+1,
		    privacy_revision=privacy_revision + CASE WHEN $2 THEN 1 ELSE 0 END,
		    updated_at=NOW()
		WHERE space_ref=$1
		RETURNING authority_revision, membership_revision, privacy_revision,
		          recipient_audience_revision, entitlement_revision`, op.space.SpaceRef, visibilityChanged).Scan(
		&next.Authority, &next.Membership, &next.Privacy, &next.RecipientAudience, &next.Entitlement,
	); err != nil {
		return nil, fmt.Errorf("advance Space settings revision: %w", err)
	}
	if _, err := insertMembershipEvent(ctx, op.tx, ledgerEntry{
		SpaceRef: op.space.SpaceRef, OrgID: op.space.OrgID, SpaceSeq: next.Authority, Action: action,
		ActorKind: ActorKindUser, ActorID: req.ActorID, IdempotencyKey: req.IdempotencyKey,
		FromRole: curJoinRole, ToRole: newJoinRole,
	}); err != nil {
		return nil, err
	}
	if err := op.tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit Space settings: %w", err)
	}
	return &SettingsResult{
		SpaceRef: op.space.SpaceRef, Visibility: newVisibility, JoinRole: newJoinRole, Changed: true,
		AuthorityRevision: next.Authority, PrivacyRevision: next.Privacy,
	}, nil
}
