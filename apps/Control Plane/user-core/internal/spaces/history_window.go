package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
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
// Off: no thread-read decision carries a floor, whatever a room's history_mode.
// It is a rollout control only, never an authorization input.
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

// applyHistoryFloor fills evidence.HistoryFloorRevision for a since_join room
// when the claim switch is on. It reads on the same pool as the evidence
// resolver; a database error fails the whole resolution (the caller maps it to
// the same unavailable response as any other resolver failure) rather than
// silently dropping the window.
func (r *Repository) applyHistoryFloor(ctx context.Context, evidence *PersonalThreadDecisionEvidence) error {
	if r == nil || !r.historyWindowClaim || evidence == nil {
		return nil
	}
	spaceRef := evidence.Membership.SpaceRef
	var mode string
	if err := r.db.Pool.QueryRow(ctx,
		`SELECT history_mode FROM registered_spaces WHERE space_ref=$1`, spaceRef).Scan(&mode); err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return nil
		}
		return fmt.Errorf("read Space history mode: %w", err)
	}
	if mode != HistoryModeSinceJoin {
		return nil
	}
	var floor *int64
	if err := r.db.Pool.QueryRow(ctx, historyFloorQuery, spaceRef, evidence.Membership.SubjectID).Scan(&floor); err != nil {
		return fmt.Errorf("derive Space history floor: %w", err)
	}
	if floor != nil && *floor > 0 {
		evidence.HistoryFloorRevision = *floor
	}
	return nil
}

// normalizeHistoryMode trims a requested history mode; empty means unchanged.
func normalizeHistoryMode(v *string) *string {
	if v == nil {
		return nil
	}
	t := strings.TrimSpace(*v)
	return &t
}
