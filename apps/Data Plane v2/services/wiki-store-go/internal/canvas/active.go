package canvas

import (
	"context"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// ActiveCanvasID returns the id of the Space's active canvas, or "" when the Space has none. It takes no lock and reads no
// content. The HTTP layer uses it to name the operation a Control decision must have signed. The write that follows is
// given this id as the canvas it must land in, and it checks that id under the Space lock, so a canvas that was purged or
// replaced in between is refused there (WS19-S8).
func (r *Repo) ActiveCanvasID(ctx context.Context, orgID, spaceRef string) (string, error) {
	if err := validateScope(orgID, spaceRef); err != nil {
		return "", err
	}
	return orgscope.InOrgScope(ctx, r.pool, orgID, func(ctx context.Context, tx pgx.Tx) (string, error) {
		cv, found, err := findActiveCanvas(ctx, tx, orgID, spaceRef, false)
		if err != nil {
			return "", err
		}
		if !found {
			return "", nil
		}
		return cv.CanvasID, nil
	})
}
