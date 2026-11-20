package canvas

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/jackc/pgx/v5"
)

// Floor is one member's history floor as the verified Control decision carries it: a D18 timestamp, a WS04-S11 revision, or
// both. A version is inside a member's window only inside every floor that is set, the AND rule Control applies to the
// transcript. The zero Floor windows nothing, so an unwindowed reader passes Floor{}.
//
// The window is applied per version (D21m). A windowed member reads and saves the current canvas: the head is always shown,
// whenever it was written, because the room works on one live document. What the window hides is history: a version written
// before the window starts is not listed, not readable on its own and not restorable. The current head is the one exception on
// every route, because the member already reads its text. A version of the history is never shown by accident: the list says
// that older versions were withheld (VersionPage.HiddenBefore), and a read or restore of one is the typed CodeHistoryFloor.
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

// admits reports whether one version, created at createdAt under the audience revision revision, lies inside the window. It
// must clear every floor that is set. The boundary is inclusive.
func (f Floor) admits(createdAt time.Time, revision int64) bool {
	if f.At != nil && createdAt.Before(*f.At) {
		return false
	}
	if f.Revision > 0 && revision < f.Revision {
		return false
	}
	return true
}

// versionInWindow reports whether one version of one canvas may be shown to a reader under floor: it lies inside the window, or
// it is the canvas's current head. A version the canvas does not hold is reported as not found (found false). It reads no
// content. A version never changes after commit, so the answer for a non-head version is stable; a caller that writes re-reads
// it under the Space lock.
func versionInWindow(ctx context.Context, tx pgx.Tx, orgID, canvasID, versionID string, floor Floor) (inWindow, found bool, err error) {
	var (
		createdAt time.Time
		revision  int64
		isHead    bool
	)
	err = tx.QueryRow(ctx, `
		SELECT v.created_at, v.audience_revision, (c.head_version_id IS NOT DISTINCT FROM v.version_id)
		FROM space_canvas_versions v
		JOIN space_canvases c ON c.canvas_id = v.canvas_id AND c.org_id = v.org_id
		WHERE v.org_id = $1 AND v.canvas_id = $2 AND v.version_id = $3`,
		orgID, canvasID, versionID).Scan(&createdAt, &revision, &isHead)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, false, nil
	}
	if err != nil {
		return false, false, fmt.Errorf("read canvas version window: %w", err)
	}
	return isHead || !floor.Set() || floor.admits(createdAt, revision), true, nil
}

// hiddenVersionError is the typed refusal of a version written before the reader's history window. It says only that this
// reader may not see or restore that version; the current canvas stays readable.
func hiddenVersionError() *Error {
	return &Error{Code: CodeHistoryFloor, Message: "this version was written before this member's history window, so it is not shown"}
}
