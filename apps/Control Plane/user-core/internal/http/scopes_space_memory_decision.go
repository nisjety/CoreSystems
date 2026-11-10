package http

import "net/http"

// EP-8 S7: scope of the memory-decision route, registered through the P-UC1
// seam. spaces:issue is the scope of the other gateway-delegated issuer routes
// and requires a verified gateway delegation. Without this rule the unmapped
// POST would fall through to users:write:self.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if method == http.MethodPost && path == "/api/v1/internal/spaces/memory-decision" {
			return "spaces:issue", true
		}
		return "", false
	})
}
