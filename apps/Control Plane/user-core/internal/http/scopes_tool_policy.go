package http

import (
	"net/http"
	"strings"
)

// WS10-S5a: scopes of the Space tool policy routes, registered through the P-UC1
// seam (feature rules run only after the core Space table did not match). Without
// a rule an unmapped PUT would fall through to users:write:self.
//
//	GET /api/v1/internal/spaces/{ref}/tool-policy                          -> spaces:issue
//	PUT /api/v1/internal/spaces/{ref}/tool-policy                          -> spaces:issue
//	PUT /api/v1/internal/spaces/{ref}/agents/{agent}/tool-policy           -> spaces:issue
//
// spaces:issue is already a delegated scope: a service credential alone never
// proves the acting user.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok {
			return "", false
		}
		parts := strings.Split(rest, "/")
		switch {
		case len(parts) == 2 && parts[1] == "tool-policy" && (method == http.MethodGet || method == http.MethodPut),
			len(parts) == 4 && parts[1] == "agents" && parts[3] == "tool-policy" && method == http.MethodPut:
			return "spaces:issue", true
		}
		return "", false
	})
}
