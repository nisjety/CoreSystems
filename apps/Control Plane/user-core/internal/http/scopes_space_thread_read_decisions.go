package http

import "net/http"

// WS11-S1: scope of the bulk thread-read-decisions route, registered through
// the P-UC1 seam. It maps to spaces:issue, the scope of the single
// thread-read-decision route, which requires a verified gateway delegation.
// Without this rule the unmapped POST would fall through to users:write:self.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if method == http.MethodPost && path == "/api/v1/internal/spaces/thread-read-decisions" {
			return "spaces:issue", true
		}
		return "", false
	})
}
