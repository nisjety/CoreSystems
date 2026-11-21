package http

import (
	"net/http"
	"strings"
)

// F-f03-01: the scope of the browser-floor read, registered through the P-UC1 seam.
// Without this rule the GET would fall through to a users:* default scope.
//
//	GET /api/v1/internal/spaces/{ref}/browser-floor   -> spaces:browser-floor:read
//
// It is a service-principal scope only: spaceScopeRequiresVerifiedDelegation does not
// list it, so org-core reads the floor with its own credential and no user delegation.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok {
			return "", false
		}
		parts := strings.Split(rest, "/")
		if len(parts) == 2 && parts[1] == "browser-floor" && method == http.MethodGet {
			return "spaces:browser-floor:read", true
		}
		return "", false
	})
}
