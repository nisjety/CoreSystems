package http

import (
	"net/http"
	"strings"
)

// WS04-S7: scope of the room archive routes, registered through the P-UC1
// seam. POST /api/v1/internal/spaces/{ref}/archive and /unarchive map to
// spaces:membership:act, which scopes_space_membership_ops.go already
// registers as a delegated scope (a service credential alone never proves the
// acting user). Without this rule an unmapped POST would fall through to
// users:write:self.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok || method != http.MethodPost {
			return "", false
		}
		parts := strings.Split(rest, "/")
		if len(parts) == 2 && (parts[1] == "archive" || parts[1] == "unarchive") {
			return spaceMembershipActScope, true
		}
		return "", false
	})
}
