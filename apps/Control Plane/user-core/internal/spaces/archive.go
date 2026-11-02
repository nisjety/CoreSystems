package spaces

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// Room archive and restore (WS04-S7, decision D-WS04-3).
//
// Archive is a Control registration_state ("archived"), orthogonal to
// Application's lifecycle. It is a read-class state: exactly RosterForSpace,
// SpacesForSubject, ResolveReadableUserMembership, ResolveSharedThreadRead-
// DecisionEvidence, ResolveSharedRetrievalDecisionEvidence and the policy fence
// (fenceActiveSpaces) keep answering for an
// archived room, while every write and agent-effect resolver stays 'active'
// only, so an archived room denies each write by default. registration_state_
// scan_test.go pins the exact set of widened predicates.
//
// Both operations run in one transaction that locks the registered_spaces row
// first (the writer's serialization point), advance authority_revision only
// (so a decision issued before the change goes stale; membership and audience
// are unchanged), append one ledger row and commit. Ids and states only: no
// content, no model call, so a Zero Data Retention organization behaves
// identically.

// RegistrationStateArchived is the Control-owned reversible archive state.
const RegistrationStateArchived = "archived"

// Typed error codes of the archive routes.
const (
	CodeRoomNotArchivable = "room_not_archivable"
	CodeRoomStateConflict = "room_state_conflict"
)

// Ledger actions of archive and restore (see migration 034's action CHECK).
const (
	ActionArchived   = "archived"
	ActionUnarchived = "unarchived"
)

// ArchiveRequest carries one archive or unarchive call. ActorID and OrgID come
// from the signed delegation, never from the body.
type ArchiveRequest struct {
	SpaceRef       string
	OrgID          string
	ActorID        string
	IdempotencyKey string
}

// ArchiveResult is the set-to-state outcome. ArchivedAt is nil when the room
// is active.
type ArchiveResult struct {
	SpaceRef          string
	State             string
	Changed           bool
	Replayed          bool
	ArchivedAt        *time.Time
	AuthorityRevision int64
}

// ArchiveSpace sets an active room to archived. Set-to-state: an already
// archived room returns changed=false.
func (r *Repository) ArchiveSpace(ctx context.Context, req ArchiveRequest) (*ArchiveResult, error) {
	return r.setArchiveState(ctx, req, true)
}

// UnarchiveSpace restores an archived room to active. Set-to-state: an
// already active room returns changed=false.
func (r *Repository) UnarchiveSpace(ctx context.Context, req ArchiveRequest) (*ArchiveResult, error) {
	return r.setArchiveState(ctx, req, false)
}

func (r *Repository) setArchiveState(ctx context.Context, req ArchiveRequest, archive bool) (*ArchiveResult, error) {
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
		return nil, fmt.Errorf("begin Space archive operation: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	space, err := registeredSpaceForUpdate(ctx, tx, req.SpaceRef)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("lock Space for archive operation: %w", err)
	}
	if space.OrgID != req.OrgID {
		return nil, roomNotFound()
	}
	// The actor must be an active member of the room and of the organization;
	// anything else is indistinguishable from an unknown room.
	var actorRole string
	err = tx.QueryRow(ctx, `
		SELECT m.role FROM space_memberships m
		WHERE m.space_ref=$1 AND m.subject_type='user' AND m.subject_id=$2 AND m.active=TRUE
		  AND EXISTS (
			SELECT 1 FROM user_org_memberships u JOIN users x ON x.id=u.user_id
			WHERE u.user_id=$2 AND u.org_id=$3 AND u.status='active' AND x.status='active'
		  )`, space.SpaceRef, req.ActorID, req.OrgID).Scan(&actorRole)
	if errors.Is(err, pgx.ErrNoRows) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, fmt.Errorf("read acting Space member: %w", err)
	}
	if space.Kind == KindPersonal {
		return nil, roomError(409, CodeRoomPersonalSpace, "a personal Space has no room settings", nil)
	}
	if space.Kind.IsDM() {
		return nil, roomError(409, CodeRoomFixedRoster, "a direct message has no room settings", nil)
	}
	if denial := CanManageRoom(actorRole); denial != nil {
		return nil, denialError(denial)
	}
	var rosterMode string
	if err := tx.QueryRow(ctx, `SELECT roster_mode FROM registered_spaces WHERE space_ref=$1`, space.SpaceRef).Scan(&rosterMode); err != nil {
		return nil, fmt.Errorf("read Space roster mode: %w", err)
	}
	if rosterMode == RosterModeOrgDerived {
		return nil, roomError(409, CodeRoomNotArchivable, "the organization room cannot be archived", nil)
	}
	if space.RegistrationState != "active" && space.RegistrationState != RegistrationStateArchived {
		return nil, roomError(409, CodeRoomStateConflict, "the room is not in a state that can be archived or restored",
			map[string]any{"state": space.RegistrationState})
	}
	var authority int64
	if err := tx.QueryRow(ctx, `
		SELECT authority_revision FROM space_authority_revisions WHERE space_ref=$1 FOR UPDATE`, space.SpaceRef).Scan(&authority); err != nil {
		return nil, fmt.Errorf("lock Space authority revision: %w", err)
	}

	action, target := ActionUnarchived, "active"
	if archive {
		action, target = ActionArchived, RegistrationStateArchived
	}
	if req.IdempotencyKey != "" {
		var priorAction, priorActor string
		err := tx.QueryRow(ctx, `
			SELECT action, actor_id FROM space_membership_events
			WHERE space_ref=$1 AND idempotency_key=$2`, space.SpaceRef, req.IdempotencyKey).Scan(&priorAction, &priorActor)
		if err == nil {
			if priorAction != action || priorActor != req.ActorID {
				return nil, roomError(409, CodeIdempotencyKeyReused, "idempotency_key was used for a different request", nil)
			}
			return archiveResult(ctx, tx, space.SpaceRef, authority, false, true)
		}
		if !errors.Is(err, pgx.ErrNoRows) {
			return nil, fmt.Errorf("read Space archive idempotency key: %w", err)
		}
	}
	if space.RegistrationState == target {
		return archiveResult(ctx, tx, space.SpaceRef, authority, false, false)
	}

	if archive {
		_, err = tx.Exec(ctx, `
			UPDATE registered_spaces
			SET registration_state='archived', archived_at=NOW(), archived_by=$2, updated_at=NOW()
			WHERE space_ref=$1`, space.SpaceRef, req.ActorID)
	} else {
		_, err = tx.Exec(ctx, `
			UPDATE registered_spaces
			SET registration_state='active', archived_at=NULL, archived_by=NULL, updated_at=NOW()
			WHERE space_ref=$1`, space.SpaceRef)
	}
	if err != nil {
		return nil, fmt.Errorf("set Space archive state: %w", err)
	}
	if err := tx.QueryRow(ctx, `
		UPDATE space_authority_revisions
		SET authority_revision=authority_revision+1, updated_at=NOW()
		WHERE space_ref=$1
		RETURNING authority_revision`, space.SpaceRef).Scan(&authority); err != nil {
		return nil, fmt.Errorf("advance Space authority revision: %w", err)
	}
	if _, err := insertMembershipEvent(ctx, tx, ledgerEntry{
		SpaceRef: space.SpaceRef, OrgID: space.OrgID, SpaceSeq: authority, Action: action,
		ActorKind: ActorKindUser, ActorID: req.ActorID, IdempotencyKey: req.IdempotencyKey,
	}); err != nil {
		return nil, err
	}
	result, err := archiveResult(ctx, tx, space.SpaceRef, authority, true, false)
	if err != nil {
		return nil, err
	}
	if err := tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit Space archive operation: %w", err)
	}
	return result, nil
}

// archiveResult reads the room's current archive facts inside the operation's
// transaction.
func archiveResult(ctx context.Context, tx pgx.Tx, spaceRef string, authority int64, changed, replayed bool) (*ArchiveResult, error) {
	var state string
	var archivedAt sql.NullTime
	if err := tx.QueryRow(ctx, `
		SELECT registration_state, archived_at FROM registered_spaces WHERE space_ref=$1`, spaceRef).Scan(&state, &archivedAt); err != nil {
		return nil, fmt.Errorf("read Space archive state: %w", err)
	}
	result := &ArchiveResult{SpaceRef: spaceRef, State: state, Changed: changed, Replayed: replayed, AuthorityRevision: authority}
	if archivedAt.Valid {
		t := archivedAt.Time.UTC()
		result.ArchivedAt = &t
	}
	return result, nil
}

// ArchivedForMember reports whether the room is archived and the subject is an
// active member of it and of the organization. It backs the typed 409
// room_archived answer of the write decisions, which would otherwise be the
// generic membership refusal. Nothing is revealed to a non-member.
func (r *Repository) ArchivedForMember(ctx context.Context, spaceRef, orgID, subjectID string) (bool, error) {
	if r == nil || r.db == nil {
		return false, fmt.Errorf("space authority repository unavailable")
	}
	var archived bool
	err := r.db.Pool.QueryRow(ctx, `
		SELECT EXISTS (
			SELECT 1 FROM registered_spaces s
			JOIN space_memberships m ON m.space_ref=s.space_ref
			WHERE s.space_ref=$1 AND s.org_id=$2 AND s.registration_state='archived'
			  AND m.subject_type='user' AND m.subject_id=$3 AND m.active=TRUE
			  AND EXISTS (
				SELECT 1 FROM user_org_memberships u
				WHERE u.user_id=$3 AND u.org_id=$2 AND u.status='active'
			  )
		)`, strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(subjectID)).Scan(&archived)
	if err != nil {
		return false, fmt.Errorf("check archived Space for member: %w", err)
	}
	return archived, nil
}
