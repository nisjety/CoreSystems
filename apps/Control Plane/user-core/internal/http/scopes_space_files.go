package http

import (
	"net/http"
	"strings"
)

// WS14-S2: scope of the room-file grant route, registered through the P-UC1
// seam. POST /api/v1/internal/spaces/file-grant maps to spaces:file-grant:write,
// held by documents-api alone. The scope is deliberately NOT a delegated scope:
// the import worker has no user bearer, so the call is proved by the
// Control-signed decision in its body (authz_owner_asserted.go). Without this
// rule an unmapped POST would fall through to users:write:self.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if method == http.MethodPost && strings.TrimSuffix(path, "/") == "/api/v1/internal/spaces/file-grant" {
			return scopeSpaceFileGrant, true
		}
		return "", false
	})
}
