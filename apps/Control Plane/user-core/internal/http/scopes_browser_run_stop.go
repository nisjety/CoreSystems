package http

import "net/http"

// D21h: scope of the browser-run-stop-decision route, registered through the
// P-UC1 seam. spaces:issue is the scope of the other gateway-delegated issuer
// routes and requires a verified gateway delegation. Without this rule the
// unmapped POST would fall through to users:write:self.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if method == http.MethodPost && path == "/api/v1/internal/spaces/browser-run-stop-decision" {
			return "spaces:issue", true
		}
		return "", false
	})
}
