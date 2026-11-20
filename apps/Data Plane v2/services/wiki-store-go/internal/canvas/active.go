package canvas

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// ActiveCanvasID returns the id of the Space's active canvas, or "" when the Space has none. It takes no lock and reads no
// content. The HTTP layer uses it to name the operation a Control decision must have signed. The write that follows is
// given this id as the canvas it must land in, and it checks that id under the Space lock, so a canvas that was purged or
// replaced in between is refused there (WS19-S8).
func (r *Repo) ActiveCanvasID(ctx context.Context, orgID, spaceRef string) (string, error) {
	p, err := r.Probe(ctx, orgID, spaceRef, "", Floor{})
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
	// InWindow reports that the probed version may be shown to the reader under the probe's floor: it lies inside the window or
	// is the current head (D21m). It is false when the version was not found.
	InWindow bool
}

// Probe reads the Space's active canvas, its purge fence and, when versionID is set, whether that version belongs to the active
// canvas and lies inside floor (the reader's history window; Floor{} windows nothing). It takes no lock and reads no content.
// The write that follows re-checks every fact under the Space lock, so a canvas that was purged or replaced in between is still
// refused there.
func (r *Repo) Probe(ctx context.Context, orgID, spaceRef, versionID string, floor Floor) (Probe, error) {
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
		p.InWindow, p.VersionFound, err = versionInWindow(ctx, tx, orgID, cv.CanvasID, versionID, floor)
		if err != nil {
			return Probe{}, fmt.Errorf("probe canvas version: %w", err)
		}
		return p, nil
	})
}
