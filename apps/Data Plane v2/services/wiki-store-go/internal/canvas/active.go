package canvas

import (
	"context"

	"github.com/jackc/pgx/v5"
	"github.com/triodelab/dataplane/shared/go/orgscope"
)

// ActiveCanvasID returns the id of the Space's active canvas, or "" when the Space has none. It takes no lock and reads no
// content. The HTTP layer uses it to name the operation a Control decision must have signed. The write that follows
// reads the canvas again under its row lock, so a head that moved in between is refused there.
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
