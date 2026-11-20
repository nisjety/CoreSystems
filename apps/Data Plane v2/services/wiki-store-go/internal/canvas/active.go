package canvas

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// ActiveCanvasID returns the id of the Space's active canvas, or "" when the Space has none. It takes no lock and reads no
// content. The HTTP layer uses it to name the operation a Control decision must have signed. The write that follows is
// given this id as the canvas it must land in, and it checks that id under the Space lock, so a canvas that was purged or
// replaced in between is refused there (WS19-S8).
func (r *Repo) ActiveCanvasID(ctx context.Context, orgID, spaceRef string) (string, error) {
	p, err := r.Probe(ctx, orgID, spaceRef, "")
	if err != nil {
		return "", err
	}
	return p.CanvasID, nil
}

// Probe is what a canvas route needs to decide its not-found answers before it decides authority (D21n). It carries ids and
// flags only, never content.
type Probe struct {
	// CanvasID is the Space's active canvas, "" when the Space has none.
	CanvasID string
	// Fenced reports that the Space was purged, so it can never take a canvas again (WS19-S8).
	Fenced bool
	// VersionFound reports that the probed version belongs to the active canvas. It is false when no version was probed.
	VersionFound bool
}

// Probe reads the Space's active canvas, its purge fence and, when versionID is set, whether that version belongs to the active
// canvas. It takes no lock and reads no content. The write that follows re-checks every fact under the Space lock, so a canvas
// that was purged or replaced in between is still refused there.
func (r *Repo) Probe(ctx context.Context, orgID, spaceRef, versionID string) (Probe, error) {
	if err := validateScope(orgID, spaceRef); err != nil {
		return Probe{}, err
	}
	return orgscope.InOrgScope(ctx, r.pool, orgID, func(ctx context.Context, tx pgx.Tx) (Probe, error) {
		cv, found, err := findActiveCanvas(ctx, tx, orgID, spaceRef, false)
		if err != nil {
			return Probe{}, err
		}
		if !found {
			fenced, err := spaceCanvasFenced(ctx, tx, orgID, spaceRef)
			if err != nil {
				return Probe{}, err
			}
			return Probe{Fenced: fenced}, nil
		}
		p := Probe{CanvasID: cv.CanvasID}
		if versionID == "" || ValidateVersionID(versionID) != nil {
			return p, nil
		}
		var one int
		err = tx.QueryRow(ctx, `SELECT 1 FROM space_canvas_versions WHERE org_id = $1 AND canvas_id = $2 AND version_id = $3`,
			orgID, cv.CanvasID, versionID).Scan(&one)
		switch {
		case err == nil:
			p.VersionFound = true
		case !errors.Is(err, pgx.ErrNoRows):
			return Probe{}, fmt.Errorf("probe canvas version: %w", err)
		}
		return p, nil
	})
}
