package users

import (
	"context"
	"errors"
)

// BrowserSiteEraser erases one member's agent-browser site choices and redacts their
// history rows in one organisation. clients.OrgCoreClient implements it over org-core's
// DELETE /internal/orgs/:orgId/members/:userId/browser-sites (F-f02-16). The call is
// idempotent, so a retried fan-out child repeats it safely. This package never imports
// the clients package; main.go passes the client in.
type BrowserSiteEraser interface {
	EraseBrowserSiteChoices(ctx context.Context, orgID, userID string) error
}

// errBrowserSiteEraserUnavailable holds a fan-out child when no eraser is wired. The child
// is retried, so the erasure cannot complete while the member's choices may remain in org-core.
var errBrowserSiteEraserUnavailable = errors.New("browser site erasure has no org-core eraser configured")

// SetBrowserSiteEraser wires the eraser that the erasure fan-out calls for each organisation
// of the erased user, before that organisation's child is published (F-f02-16). There is no
// switch: erasure is never gated. Pass a nil interface, never a typed nil. With no eraser the
// fan-out fails closed: no child is published or completed, so the erasure stays open until
// the eraser is configured.
func (s *Service) SetBrowserSiteEraser(eraser BrowserSiteEraser) {
	s.siteEraser = eraser
}

// eraseChildBrowserSiteChoices runs the org-core erase for one fan-out child, for the user
// being erased. A failure is returned, so the child is held for retry and not published.
func (s *Service) eraseChildBrowserSiteChoices(ctx context.Context, operation ErasureOperation, child ErasureFanoutChild) error {
	if s.siteEraser == nil {
		return errBrowserSiteEraserUnavailable
	}
	return s.siteEraser.EraseBrowserSiteChoices(ctx, child.OrgID, operation.UserID)
}
