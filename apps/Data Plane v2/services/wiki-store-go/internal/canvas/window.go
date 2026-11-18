package canvas

import (
	"context"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
)

// Floor is one member's history floor as the verified Control decision carries it: a D18 timestamp, a WS04-S11 revision, or
// both. A canvas is shown to a member only inside every floor that is set, the AND rule Control applies to the transcript. The
// zero Floor windows nothing, so an unwindowed reader passes Floor{}.
//
// The window is applied to the whole canvas, not to single versions. Each version stores the whole document, so a version
// written after the floor can still carry text that was written before it: a save edits a document that already holds that
// text. Only a canvas whose every version was written inside the window can be shown without showing pre-floor text. That holds
// when the canvas was created inside the window and no version began before the window started; a canvas created inside the
// window can still fail the second test, which is the conservative direction. Any other canvas is refused to a windowed reader
// with CodeHistoryFloor, for its head, its versions and its version list alike. Versions are never shown one by one.
type Floor struct {
	// At is the instant the window starts at. Nil means no timestamp floor. A version's created_at is the start of the transaction
	// that wrote it, so a version that began before the floor counts as before it. That is the direction that hides.
	At *time.Time
	// Revision is the first recipient-audience revision the window starts at. Zero means no revision floor. A version's
	// audience_revision is the revision of the decision it was written under.
	Revision int64
}

// Set reports whether the floor windows anything.
func (f Floor) Set() bool { return f.At != nil || f.Revision > 0 }

// admits reports whether a canvas whose earliest version was created at oldestAt, under the lowest audience revision
// oldestRevision among its versions, lies wholly inside the window. Both minima must clear every floor that is set.
func (f Floor) admits(oldestAt time.Time, oldestRevision int64) bool {
	if f.At != nil && oldestAt.Before(*f.At) {
		return false
	}
	if f.Revision > 0 && oldestRevision < f.Revision {
		return false
	}
	return true
}

// windowAdmits reports whether every version of one canvas lies inside the floor. It is one aggregate statement, run in the
// same transaction as the read it guards. The callers order the two so that each version they return was committed before this
// statement starts: a canvas's head is committed before the canvas row names it, and ListVersions and GetVersion read their rows
// before the check. A version never changes after commit, so the minima cover every version the caller returns. A canvas with no
// versions is not admitted.
func windowAdmits(ctx context.Context, tx pgx.Tx, orgID, canvasID string, floor Floor) (bool, error) {
	var (
		oldestAt       *time.Time
		oldestRevision *int64
		versions       int64
	)
	if err := tx.QueryRow(ctx,
		`SELECT min(created_at), min(audience_revision), count(*)
		FROM space_canvas_versions WHERE org_id = $1 AND canvas_id = $2`,
		orgID, canvasID).Scan(&oldestAt, &oldestRevision, &versions); err != nil {
		return false, fmt.Errorf("read canvas history window: %w", err)
	}
	if versions == 0 || oldestAt == nil || oldestRevision == nil {
		return false, nil
	}
	return floor.admits(*oldestAt, *oldestRevision), nil
}

// hiddenCanvasError is the typed refusal of a canvas that predates the reader's history window. It does not say whether
// the canvas has text, only that this reader may not see it.
func hiddenCanvasError() *Error {
	return &Error{Code: CodeHistoryFloor, Message: "the canvas was created before this member's history window, so it is not shown"}
}
