package spaces

import (
	"context"
	"strings"
)

// History window (WS04-S11). A room's history_mode is "full" (default: every
// member reads the whole transcript) or "since_join" (a member reads only what
// was posted from the recipient-audience revision at which they last joined).
//
// The window is a PRESENTATION rule that Session Core applies through an
// optional, Control-signed floor on the thread-read decision. It is never an
// authorization input: it can only narrow what an already-authorised read
// returns, and an old Session Core that ignores the claim shows full history
// (documented fail-open on a presentation rule, never on authority).

const (
	HistoryModeFull      = "full"
	HistoryModeSinceJoin = "since_join"
)

// ValidHistoryMode reports whether v is one of the two closed history modes.
func ValidHistoryMode(v string) bool {
	return v == HistoryModeFull || v == HistoryModeSinceJoin
}

// SetHistoryWindowClaim turns the SPACE_HISTORY_WINDOW_CLAIM switch on or off.
// Off: a since_join ROOM signs no floor, whatever its history_mode. A per-member
// window (member_history_window.go, D18) is enforced whatever this switch says: a
// hide must never depend on a rollout control. It is never an authorization input.
func (r *Repository) SetHistoryWindowClaim(enabled bool) {
	r.historyWindowClaim = enabled
}

// HistoryWindowClaimEnabled reports the switch.
func (r *Repository) HistoryWindowClaimEnabled() bool { return r != nil && r.historyWindowClaim }

// historyFloorQuery derives a member's floor from the audience snapshots with
// no membership column: the first audience revision after the latest revision
// whose snapshot did NOT contain the subject. A subject present in every
// snapshot since registration has no floor (NULL: the whole transcript).
// Revisions are compared, never counted, so gaps in the sequence are harmless.
const historyFloorQuery = `
	SELECT (
		SELECT min(a.revision)
		FROM space_recipient_audiences a
		WHERE a.space_ref=$1
		  AND a.revision > gap.last_missing
	)
	FROM (
		SELECT max(a.revision) AS last_missing
		FROM space_recipient_audiences a
		WHERE a.space_ref=$1
		  AND NOT EXISTS (
			SELECT 1 FROM space_recipient_audience_members am
			WHERE am.space_ref=a.space_ref AND am.revision=a.revision AND am.subject_id=$2
		  )
	) gap`

// applyHistoryFloor fills the history floor a thread read or a canvas read carries.
// It delegates to resolveHistoryFloor, the one derivation the transcript, the
// canvas and the membership feed share (D18). The revision floor is what WS04-S11
// always signed (a since_join room, behind the claim switch). The timestamp floor
// is set only for a per-member row, so the signed decision gains it additively and
// Session Core enforces it whatever its own switch says. A database error, or a
// row that cannot be read, fails the resolution: it is never read as full history.
func (r *Repository) applyHistoryFloor(ctx context.Context, evidence *PersonalThreadDecisionEvidence) error {
	if evidence == nil {
		return nil
	}
	floor, err := r.resolveHistoryFloor(ctx, evidence.Membership.SpaceRef, evidence.Membership.SubjectID)
	if err != nil {
		return err
	}
	evidence.HistoryFloorRevision = floor.Revision
	if floor.Source == historyFloorSourceMember {
		evidence.HistoryFloorAt = floor.At
	}
	return nil
}

// ApplyReadHistoryFloor resolves a member's floor onto evidence that a handler gathered through a resolver which
// does not apply one (D18). It is for a shared Space only: a personal Space has one member and no window, so its
// evidence is left as it is. A resolution error is returned, so the caller refuses the decision (fail closed).
func (r *Repository) ApplyReadHistoryFloor(ctx context.Context, evidence *PersonalThreadDecisionEvidence) error {
	if evidence == nil || evidence.Membership.Kind == KindPersonal {
		return nil
	}
	return r.applyHistoryFloor(ctx, evidence)
}

// hasHistoryFloor reports whether the evidence carries any floor: a WS04-S11 revision floor or a D18 timestamp floor.
func (e PersonalThreadDecisionEvidence) hasHistoryFloor() bool {
	return e.HistoryFloorRevision > 0 || e.HistoryFloorAt != nil
}

// carryHistoryFloor copies the member's floor onto a decision whose verifier applies it as a presentation narrowing
// (D18). Each issuer that calls it opts in explicitly; a decision that does not call it carries no floor, so the
// personal and own-scheduled decisions stay floor-free. The floor is additive and in no payload digest.
func carryHistoryFloor(decision Decision, evidence PersonalThreadDecisionEvidence) Decision {
	decision.HistoryFloorRevision = evidence.HistoryFloorRevision
	decision.HistoryFloorAt = nil
	if evidence.HistoryFloorAt != nil {
		at := evidence.HistoryFloorAt.UTC()
		decision.HistoryFloorAt = &at
	}
	return decision
}

// normalizeHistoryMode trims a requested history mode; empty means unchanged.
func normalizeHistoryMode(v *string) *string {
	if v == nil {
		return nil
	}
	t := strings.TrimSpace(*v)
	return &t
}
