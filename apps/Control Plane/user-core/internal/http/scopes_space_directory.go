package http

import (
	"net/http"
	"strings"
)

// WS04-S8: scopes of the room join, settings and directory-policy routes,
// registered through the P-UC1 seam. /directory/rooms is already covered by the
// directory prefix rule (directory:read:self). Without these rules an unmapped
// POST, PATCH or PUT would fall through to users:write:self.
//
//	POST|PATCH /api/v1/internal/spaces/{ref}/(join|settings) -> spaces:membership:act (delegated)
//	PUT /api/v1/internal/spaces/directory-policy             -> spaces:policy:write
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok {
			return "", false
		}
		parts := strings.Split(rest, "/")
		switch {
		case len(parts) == 1 && parts[0] == "directory-policy" && method == http.MethodPut:
			return "spaces:policy:write", true
		case len(parts) == 2 && parts[1] == "join" && method == http.MethodPost,
			len(parts) == 2 && parts[1] == "settings" && method == http.MethodPatch:
			return spaceMembershipActScope, true
		}
		return "", false
	})
}
