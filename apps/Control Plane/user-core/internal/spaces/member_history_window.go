package spaces

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/jackc/pgx/v5"
)

// Per-member room history visibility (D18, owner decision confirmed twice).
//
// The owner's rule: a member sees the room's past by default. An invite can choose
// "don't share past conversation": the member joins, and everything posted before
// their join is hidden. A room admin can later move one member to full history, to
// history from a chosen date, or back to join-only. A row overrides the room's
// history_mode for its member and leaves the room's own mode unchanged. With no row,
// the room decides exactly as WS04-S11 does today.
//
// The window is a presentation narrowing of an already-authorised read. It can only
// HIDE. It never widens an ACL and it is never an authorization input. The floor is
// resolved once here, and the transcript, the canvas and the membership feed all read
// that one resolution. A row the resolver cannot read fails the read closed: it is
// never treated as full history.
//
// Every change is a Control write in one transaction with one user_audit_outbox row.
// The audit payload is content-free: ids, modes, instants and revisions only. A row
// holds a mode and a timestamp and no content, so it is allowed under every Zero Data
// Retention posture.

// Per-member modes. full and since_join reuse the room-level literals. room is the
// explicit "no override" row an invite writes when it shares the past again.
const (
	MemberHistoryModeRoom      = "room"
	MemberHistoryModeFull      = HistoryModeFull
	MemberHistoryModeSinceJoin = HistoryModeSinceJoin
	MemberHistoryModeSinceDate = "since_date"
)

// Floor sources. A per-member row decides under its own source, and only that source's
// timestamp reaches the signed decision. The room source keeps the WS04-S11 shape.
const (
	historyFloorSourceRoom   = "room"
	historyFloorSourceMember = "member"
)

// Audit sources of one change: an admin's write, or an invite's choice on the add path.
const (
	historyWindowSourceAdmin  = "admin"
	historyWindowSourceInvite = "invite"
)

// Stable error codes of the per-member history window (D18). Part of the gateway and SPA
// contract; never rename one.
const (
	CodeMemberHistoryWindowInvalid     = "member_history_window_invalid"
	CodeMemberHistoryWindowUnavailable = "member_history_window_unavailable"
)

// AuditSubjectMemberHistoryWindowChanged is the user_audit_outbox subject of one per-member
// visibility change, inside user-core's authority prefix (nats.conf publishes
// verevon.audit.v2.control.user-core.>).
const (
	AuditSubjectMemberHistoryWindowChanged = "verevon.audit.v2.control.user-core.space_member_history_window_changed"
	auditEventMemberHistoryWindowChanged   = "space_member_history_window_changed"
)

// errMemberHistoryWindowCorrupt refuses a stored row the resolver cannot read. The read is
// answered unavailable: a hide-history row is never read as full history.
var errMemberHistoryWindowCorrupt = errors.New("space member history window row is unreadable")

// SetMemberHistoryWindows turns the SPACE_MEMBER_HISTORY_WINDOWS switch on or off. It gates the
// writes only: while it is off, an invite's "don't share" choice and the admin change answer
// member_history_window_unavailable, so the write path fails closed. Rows already stored keep
// being enforced, because a rollout switch must never undo a hide.
func (r *Repository) SetMemberHistoryWindows(enabled bool) {
	r.memberHistoryWindows = enabled
}

// MemberHistoryWindowsEnabled reports the switch.
func (r *Repository) MemberHistoryWindowsEnabled() bool {
	return r != nil && r.memberHistoryWindows
}

// HistoryFloor is the presentation floor of one member's reads. A row is visible only when it
// satisfies every floor that is set.
//
//   - Revision is the first recipient-audience revision the member may read from; 0 means none.
//   - At is the first instant the member may read from; nil means none.
//   - Source is "member" when a per-member row decided, and "room" when the room's history_mode did.
type HistoryFloor struct {
	Revision int64
	At       *time.Time
	Source   string
}

// memberWindowState is one member's stored window, read under the row lock.
type memberWindowState struct {
	Exists   bool
	Mode     string
	SinceAt  *time.Time
	Revision int64
}

// MemberHistoryWindowView is the window one member reads under, as responses show it. Source is
// "member" when the member's row decides. It is "room" when the room's history_mode does: Mode then
// names that room mode, and Revision is 0 or the revision of an explicit reset.
type MemberHistoryWindowView struct {
	Source   string     `json:"source"`
	Mode     string     `json:"mode"`
	SinceAt  *time.Time `json:"since_at"`
	Revision int64      `json:"revision"`
}

// resolveHistoryFloor derives one member's floor. It is the single source the transcript
// decision, the canvas decision and the membership feed all read.
//
//   - A per-member row (full, since_join or since_date) decides for its member, whatever the
//     room's history_mode is and whatever SPACE_HISTORY_WINDOW_CLAIM says. A hide must never
//     depend on a rollout switch.
//   - No row, or a 'room' row: the room decides, exactly as WS04-S11 does today. Only a since_join
//     room floors, and only while SPACE_HISTORY_WINDOW_CLAIM is on, on the revision alone.
func (r *Repository) resolveHistoryFloor(ctx context.Context, spaceRef, subjectID string) (HistoryFloor, error) {
	if r == nil || r.db == nil {
		return HistoryFloor{}, fmt.Errorf("space authority repository unavailable")
	}
	var mode string
	var sinceAt *time.Time
	err := r.db.Pool.QueryRow(ctx, `
		SELECT mode, since_at FROM space_member_history_windows
		WHERE space_ref=$1 AND subject_id=$2`, spaceRef, subjectID).Scan(&mode, &sinceAt)
	switch {
	case err == nil && mode != MemberHistoryModeRoom:
		return r.memberWindowFloor(ctx, spaceRef, subjectID, mode, sinceAt)
	case err != nil && !errors.Is(err, pgx.ErrNoRows):
		return HistoryFloor{}, fmt.Errorf("read Space member history window: %w", err)
	}
	return r.roomHistoryFloor(ctx, spaceRef, subjectID)
}

// memberWindowFloor is the floor a per-member row sets.
func (r *Repository) memberWindowFloor(ctx context.Context, spaceRef, subjectID, mode string, sinceAt *time.Time) (HistoryFloor, error) {
	switch mode {
	case MemberHistoryModeFull:
		return HistoryFloor{}, nil
	case MemberHistoryModeSinceDate:
		if sinceAt == nil {
			return HistoryFloor{}, errMemberHistoryWindowCorrupt
		}
		at := sinceAt.UTC()
		return HistoryFloor{At: &at, Source: historyFloorSourceMember}, nil
	case MemberHistoryModeSinceJoin:
		return r.joinFloor(ctx, spaceRef, subjectID, historyFloorSourceMember)
	}
	return HistoryFloor{}, errMemberHistoryWindowCorrupt
}

// roomHistoryFloor is WS04-S11 unchanged: a since_join room floors on the revision, behind the claim switch.
func (r *Repository) roomHistoryFloor(ctx context.Context, spaceRef, subjectID string) (HistoryFloor, error) {
	if !r.historyWindowClaim {
		return HistoryFloor{}, nil
	}
	var roomMode string
	err := r.db.Pool.QueryRow(ctx, `SELECT history_mode FROM registered_spaces WHERE space_ref=$1`, spaceRef).Scan(&roomMode)
	if errors.Is(err, pgx.ErrNoRows) {
		return HistoryFloor{}, nil
	}
	if err != nil {
		return HistoryFloor{}, fmt.Errorf("read Space history mode: %w", err)
	}
	if roomMode != HistoryModeSinceJoin {
		return HistoryFloor{}, nil
	}
	return r.joinFloor(ctx, spaceRef, subjectID, historyFloorSourceRoom)
}

// joinFloor derives a since_join floor. The revision is the audience revision the member first
// appears in after their last absence (historyFloorQuery, WS04-S11). The instant is the member's
// latest join in the membership ledger, so the hide and the feed agree with the join. With no
// ledger join (a member seeded declaratively) the instant is the registration time of that revision.
func (r *Repository) joinFloor(ctx context.Context, spaceRef, subjectID, source string) (HistoryFloor, error) {
	var revision *int64
	if err := r.db.Pool.QueryRow(ctx, historyFloorQuery, spaceRef, subjectID).Scan(&revision); err != nil {
		return HistoryFloor{}, fmt.Errorf("derive Space history floor: %w", err)
	}
	floor := HistoryFloor{Source: source}
	if revision != nil && *revision > 0 {
		floor.Revision = *revision
	}
	var joinedAt *time.Time
	if err := r.db.Pool.QueryRow(ctx, `
		SELECT max(created_at) FROM space_membership_events
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2
		  AND action IN ('member_added', 'member_joined')`, spaceRef, subjectID).Scan(&joinedAt); err != nil {
		return HistoryFloor{}, fmt.Errorf("read Space member join instant: %w", err)
	}
	if joinedAt == nil && floor.Revision > 0 {
		var registeredAt time.Time
		if err := r.db.Pool.QueryRow(ctx, `
			SELECT registered_at FROM space_recipient_audiences WHERE space_ref=$1 AND revision=$2`,
			spaceRef, floor.Revision).Scan(&registeredAt); err != nil {
			return HistoryFloor{}, fmt.Errorf("read Space history floor instant: %w", err)
		}
		joinedAt = &registeredAt
	}
	if joinedAt != nil {
		at := joinedAt.UTC()
		floor.At = &at
	}
	return floor, nil
}

// NormalizeMemberHistoryWindow validates one requested per-member window and returns its canonical form:
// the mode, and since_at in UTC for since_date only. A since_date may not start in the future. 'room' is
// not a requested mode: only an invite writes it.
func NormalizeMemberHistoryWindow(mode string, sinceAt *time.Time, now time.Time) (string, *time.Time, error) {
	mode = strings.TrimSpace(mode)
	switch mode {
	case MemberHistoryModeFull, MemberHistoryModeSinceJoin:
		if sinceAt != nil {
			return "", nil, memberHistoryWindowInvalid("since_at", "since_at is only allowed with since_date")
		}
		return mode, nil, nil
	case MemberHistoryModeSinceDate:
		if sinceAt == nil || sinceAt.IsZero() {
			return "", nil, memberHistoryWindowInvalid("since_at", "since_at is required with since_date")
		}
		at := sinceAt.UTC()
		if at.After(now.UTC()) {
			return "", nil, memberHistoryWindowInvalid("since_at", "since_at cannot be in the future")
		}
		return mode, &at, nil
	}
	return "", nil, memberHistoryWindowInvalid("mode", "mode must be full, since_join or since_date")
}

func memberHistoryWindowInvalid(field, message string) *RoomError {
	return roomError(422, CodeMemberHistoryWindowInvalid, message, map[string]any{"field": field})
}

// MemberHistoryWindowRequest is one admin change of one member's visibility. OrgID and ActorID come from
// the verified gateway delegation, never from a body. Now is the clock; zero means the wall clock.
type MemberHistoryWindowRequest struct {
	SpaceRef         string
	OrgID            string
	ActorID          string
	SubjectID        string
	Mode             string
	SinceAt          *time.Time
	ExpectedRevision *int64
	Now              time.Time
}

// MemberHistoryWindowResult is the window after a change or a read. Changed is false when the member
// already read under the requested window, and then nothing was written.
type MemberHistoryWindowResult struct {
	SpaceRef  string
	SubjectID string
	Window    MemberHistoryWindowView
	Changed   bool
}

// SetMemberHistoryWindow sets one member's visibility to full, since_join or since_date. Only a room manager or
// owner may (CanManageRoom), and the member must be an active human member of the room. One transaction locks
// the room, the actor and the member's row, writes the row and enqueues its audit row. It is set-to-state: the
// same window answers changed=false and writes nothing.
func (r *Repository) SetMemberHistoryWindow(ctx context.Context, req MemberHistoryWindowRequest) (*MemberHistoryWindowResult, error) {
	member := MemberOpRequest{SpaceRef: req.SpaceRef, OrgID: req.OrgID, ActorID: req.ActorID, SubjectID: req.SubjectID}
	if err := normalizeMemberRequest(&member); err != nil {
		return nil, err
	}
	now := req.Now
	if now.IsZero() {
		now = time.Now().UTC()
	}
	mode, sinceAt, err := NormalizeMemberHistoryWindow(req.Mode, req.SinceAt, now)
	if err != nil {
		return nil, err
	}
	if !r.MemberHistoryWindowsEnabled() {
		return nil, roomError(503, CodeMemberHistoryWindowUnavailable, "per-member history windows are not enabled", nil)
	}
	if r.policyAudit == nil {
		return nil, roomError(503, CodeSpaceAuthorityUnavailable, "space authority is unavailable", nil)
	}
	op, err := r.beginRoomOp(ctx, member)
	if err != nil {
		return nil, err
	}
	defer func() { _ = op.tx.Rollback(ctx) }()
	if err := op.refuseDerivedOrArchived(); err != nil {
		return nil, err
	}
	if denial := CanManageRoom(op.actorRole); denial != nil {
		return nil, denialError(denial)
	}
	if err := op.requireActiveMember(ctx, member.SubjectID); err != nil {
		return nil, err
	}
	current, err := lockMemberHistoryWindow(ctx, op.tx, op.space.SpaceRef, member.SubjectID)
	if err != nil {
		return nil, err
	}
	if req.ExpectedRevision != nil && *req.ExpectedRevision != current.Revision {
		return nil, roomError(409, CodeRoomMemberChanged, "the member's history window changed since it was read",
			map[string]any{"current_revision": current.Revision})
	}
	if current.Exists && current.Mode == mode && sameInstant(current.SinceAt, sinceAt) {
		return &MemberHistoryWindowResult{
			SpaceRef: op.space.SpaceRef, SubjectID: member.SubjectID,
			Window: MemberHistoryWindowView{Source: historyFloorSourceMember, Mode: mode, SinceAt: sinceAt, Revision: current.Revision},
		}, nil
	}
	revision, err := r.writeMemberHistoryWindow(ctx, op.tx, memberWindowChange{
		OrgID: op.space.OrgID, SpaceRef: op.space.SpaceRef, SubjectID: member.SubjectID,
		ActorID: op.actorID, ActorRole: op.actorRole, Source: historyWindowSourceAdmin,
		From: current, ToMode: mode, ToSinceAt: sinceAt,
	}, now)
	if err != nil {
		return nil, err
	}
	if err := op.tx.Commit(ctx); err != nil {
		return nil, fmt.Errorf("commit Space member history window: %w", err)
	}
	return &MemberHistoryWindowResult{
		SpaceRef: op.space.SpaceRef, SubjectID: member.SubjectID, Changed: true,
		Window: MemberHistoryWindowView{Source: historyFloorSourceMember, Mode: mode, SinceAt: sinceAt, Revision: revision},
	}, nil
}

// MemberHistoryWindowFor reads one member's window for a room admin. Room managers and owners only; the
// target must be an active human member. A caller who is not a member of the room gets the same answer as
// an unknown room.
func (r *Repository) MemberHistoryWindowFor(ctx context.Context, spaceRef, orgID, actorID, subjectID string) (*MemberHistoryWindowResult, error) {
	if r == nil || r.db == nil {
		return nil, roomError(503, CodeSpaceAuthorityUnavailable, "space authority is unavailable", nil)
	}
	actor, err := r.ResolveReadableUserMembership(ctx, strings.TrimSpace(spaceRef), strings.TrimSpace(orgID), strings.TrimSpace(actorID))
	if errors.Is(err, ErrNoCurrentMembership) {
		return nil, roomNotFound()
	}
	if err != nil {
		return nil, err
	}
	if denial := CanManageRoom(actor.Role); denial != nil {
		return nil, denialError(denial)
	}
	subjectID = strings.TrimSpace(subjectID)
	var active bool
	err = r.db.Pool.QueryRow(ctx, `
		SELECT active FROM space_memberships
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2`, actor.SpaceRef, subjectID).Scan(&active)
	if errors.Is(err, pgx.ErrNoRows) || (err == nil && !active) {
		return nil, roomError(404, CodeRoomMemberNotFound, "member not found", nil)
	}
	if err != nil {
		return nil, fmt.Errorf("read target Space member: %w", err)
	}
	var state memberWindowState
	var sinceAt *time.Time
	err = r.db.Pool.QueryRow(ctx, `
		SELECT mode, since_at, revision FROM space_member_history_windows
		WHERE space_ref=$1 AND subject_id=$2`, actor.SpaceRef, subjectID).Scan(&state.Mode, &sinceAt, &state.Revision)
	switch {
	case err == nil:
		state.Exists, state.SinceAt = true, sinceAt
	case !errors.Is(err, pgx.ErrNoRows):
		return nil, fmt.Errorf("read Space member history window: %w", err)
	}
	return &MemberHistoryWindowResult{
		SpaceRef: actor.SpaceRef, SubjectID: subjectID,
		Window: windowView(state, actor.HistoryMode),
	}, nil
}

// windowView renders a stored window for a response: the member's own row when it is not 'room', and
// otherwise the room's mode.
func windowView(state memberWindowState, roomMode string) MemberHistoryWindowView {
	if state.Exists && state.Mode != MemberHistoryModeRoom {
		return MemberHistoryWindowView{Source: historyFloorSourceMember, Mode: state.Mode, SinceAt: state.SinceAt, Revision: state.Revision}
	}
	return MemberHistoryWindowView{Source: historyFloorSourceRoom, Mode: roomMode, Revision: state.Revision}
}

// requireActiveMember refuses a subject who is not an active human member of the room, as unknown.
func (op *roomOp) requireActiveMember(ctx context.Context, subjectID string) error {
	var active bool
	err := op.tx.QueryRow(ctx, `
		SELECT active FROM space_memberships
		WHERE space_ref=$1 AND subject_type='user' AND subject_id=$2 FOR UPDATE`,
		op.space.SpaceRef, subjectID).Scan(&active)
	if errors.Is(err, pgx.ErrNoRows) || (err == nil && !active) {
		return roomError(404, CodeRoomMemberNotFound, "member not found", nil)
	}
	if err != nil {
		return fmt.Errorf("read target Space member: %w", err)
	}
	return nil
}

// applyInvitedMemberWindow applies an invite's choice to the member the add has just made active, inside the
// add's own transaction. notShared writes since_join: the member joins and does not see the past before their
// join. Otherwise any row is reset to 'room', so the room's history_mode decides exactly as today.
func (op *roomOp) applyInvitedMemberWindow(ctx context.Context, r *Repository, subjectID string, notShared bool) error {
	current, err := lockMemberHistoryWindow(ctx, op.tx, op.space.SpaceRef, subjectID)
	if err != nil {
		return err
	}
	change := memberWindowChange{
		OrgID: op.space.OrgID, SpaceRef: op.space.SpaceRef, SubjectID: subjectID,
		ActorID: op.actorID, ActorRole: op.actorRole, Source: historyWindowSourceInvite, From: current,
	}
	if notShared {
		if current.Exists && current.Mode == MemberHistoryModeSinceJoin {
			return nil
		}
		change.ToMode = MemberHistoryModeSinceJoin
	} else {
		if !current.Exists || current.Mode == MemberHistoryModeRoom {
			return nil
		}
		change.ToMode = MemberHistoryModeRoom
	}
	_, err = r.writeMemberHistoryWindow(ctx, op.tx, change, time.Now().UTC())
	return err
}

// historyWindowView reads the window the member reads under, inside the add's transaction.
func (op *roomOp) historyWindowView(ctx context.Context, subjectID string) (MemberHistoryWindowView, error) {
	current, err := lockMemberHistoryWindow(ctx, op.tx, op.space.SpaceRef, subjectID)
	if err != nil {
		return MemberHistoryWindowView{}, err
	}
	var roomMode string
	if err := op.tx.QueryRow(ctx, `SELECT history_mode FROM registered_spaces WHERE space_ref=$1`,
		op.space.SpaceRef).Scan(&roomMode); err != nil {
		return MemberHistoryWindowView{}, fmt.Errorf("read Space history mode: %w", err)
	}
	return windowView(current, roomMode), nil
}

// lockMemberHistoryWindow reads one member's stored window under a row lock (none when the member has no row).
func lockMemberHistoryWindow(ctx context.Context, tx pgx.Tx, spaceRef, subjectID string) (memberWindowState, error) {
	var state memberWindowState
	var sinceAt *time.Time
	err := tx.QueryRow(ctx, `
		SELECT mode, since_at, revision FROM space_member_history_windows
		WHERE space_ref=$1 AND subject_id=$2 FOR UPDATE`, spaceRef, subjectID).Scan(&state.Mode, &sinceAt, &state.Revision)
	if errors.Is(err, pgx.ErrNoRows) {
		return memberWindowState{}, nil
	}
	if err != nil {
		return memberWindowState{}, fmt.Errorf("lock Space member history window: %w", err)
	}
	state.Exists, state.SinceAt = true, sinceAt
	return state, nil
}

// memberWindowChange is one change to a member's row, with the facts its audit row carries.
type memberWindowChange struct {
	OrgID     string
	SpaceRef  string
	SubjectID string
	ActorID   string
	ActorRole string
	Source    string
	From      memberWindowState
	ToMode    string
	ToSinceAt *time.Time
}

// writeMemberHistoryWindow upserts one member's row and enqueues its audit row on the caller's transaction,
// returning the new revision. The caller holds the row lock and has decided that the change is due.
func (r *Repository) writeMemberHistoryWindow(ctx context.Context, tx pgx.Tx, change memberWindowChange, now time.Time) (int64, error) {
	if r == nil || r.policyAudit == nil {
		return 0, roomError(503, CodeSpaceAuthorityUnavailable, "space authority is unavailable", nil)
	}
	var revision int64
	if err := tx.QueryRow(ctx, `
		INSERT INTO space_member_history_windows (space_ref, subject_id, mode, since_at, set_by, revision, updated_at)
		VALUES ($1,$2,$3,$4,$5,1,NOW())
		ON CONFLICT (space_ref, subject_id) DO UPDATE
		SET mode=EXCLUDED.mode, since_at=EXCLUDED.since_at, set_by=EXCLUDED.set_by,
		    revision=space_member_history_windows.revision+1, updated_at=NOW()
		RETURNING revision`,
		change.SpaceRef, change.SubjectID, change.ToMode, change.ToSinceAt, change.ActorID,
	).Scan(&revision); err != nil {
		return 0, fmt.Errorf("write Space member history window: %w", err)
	}
	payload, err := buildMemberHistoryWindowAuditPayload(change, revision, now)
	if err != nil {
		return 0, err
	}
	if err := r.policyAudit.EnqueueAuditTx(ctx, tx, memberHistoryWindowEventID(change.SpaceRef, change.SubjectID, revision),
		AuditSubjectMemberHistoryWindowChanged, payload); err != nil {
		return 0, fmt.Errorf("enqueue member history window audit: %w", err)
	}
	return revision, nil
}

// memberHistoryWindowEventID is the audit event id of one change. Revisions only ever advance (rows are never
// deleted), so the id is unique per member row.
func memberHistoryWindowEventID(spaceRef, subjectID string, revision int64) string {
	return fmt.Sprintf("member-history:%s:%s:%d", spaceRef, subjectID, revision)
}

// buildMemberHistoryWindowAuditPayload renders the audit event in the events.AuditEvent shape the org-agent audit
// uses. Ids, modes, instants and revisions only: no names, no content.
func buildMemberHistoryWindowAuditPayload(change memberWindowChange, revisionAfter int64, now time.Time) ([]byte, error) {
	fromMode := MemberHistoryModeRoom
	if change.From.Exists {
		fromMode = change.From.Mode
	}
	payload := map[string]any{
		"event_id":    memberHistoryWindowEventID(change.SpaceRef, change.SubjectID, revisionAfter),
		"occurred_at": now.UTC().Format(time.RFC3339Nano),
		"org_id":      change.OrgID,
		"user_id":     change.ActorID,
		"actor_role":  change.ActorRole,
		"plane":       "control",
		"producer":    "user-core",
		"event":       auditEventMemberHistoryWindowChanged,
		"subject":     "org:" + change.OrgID,
		"resource_id": change.SpaceRef,
		"outcome":     "ok",
		"details": map[string]any{
			"subject_id":      change.SubjectID,
			"source":          change.Source,
			"from_mode":       fromMode,
			"to_mode":         change.ToMode,
			"from_since_at":   instantOrNil(change.From.SinceAt),
			"to_since_at":     instantOrNil(change.ToSinceAt),
			"revision_before": change.From.Revision,
			"revision_after":  revisionAfter,
		},
	}
	encoded, err := json.Marshal(payload)
	if err != nil {
		return nil, fmt.Errorf("encode member history window audit event: %w", err)
	}
	return encoded, nil
}

// instantOrNil renders an optional instant for an audit payload: JSON null when absent.
func instantOrNil(t *time.Time) any {
	if t == nil {
		return nil
	}
	return t.UTC().Format(time.RFC3339Nano)
}

// sameInstant reports whether two optional instants are both absent or the same instant.
func sameInstant(a, b *time.Time) bool {
	if a == nil || b == nil {
		return a == b
	}
	return a.Equal(*b)
}
