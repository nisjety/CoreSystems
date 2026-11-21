package spaces

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/jackc/pgx/v5"
)

// BrowserFloor reads one Space's browser_entitled bit for org-core's browser-site
// resolve (F-f03-01, DEC-19 D9). user-core owns the Space and its floor, so org-core reads
// the answer through the internal browser-floor route and never through these tables.
//
// The answer is the stored column and nothing else. A Space that is not an active
// registration of the organisation, and an organisation with no effect-policy row, are
// not entitled: (false, nil). Only a database failure is an error, and the caller refuses
// on it, so an unreadable floor narrows.
func (r *Repository) BrowserFloor(ctx context.Context, orgID, spaceRef string) (bool, error) {
	var entitled bool
	err := r.db.Pool.QueryRow(ctx, `
SELECT p.browser_entitled
FROM registered_spaces s
JOIN space_effect_policies p ON p.org_id = s.org_id
WHERE s.space_ref = $1 AND s.org_id = $2 AND s.registration_state = 'active'`,
		strings.TrimSpace(spaceRef), strings.TrimSpace(orgID),
	).Scan(&entitled)
	if errors.Is(err, pgx.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("read Space browser floor: %w", err)
	}
	return entitled, nil
}
