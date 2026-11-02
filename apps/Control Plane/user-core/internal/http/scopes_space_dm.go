package http

import "net/http"

// WS12-S1: POST /api/v1/internal/spaces/dm-eligibility maps to spaces:resolve,
// which is already a verified-delegation scope (the acting user is the caller
// of the pre-check, never a body field). Without this rule an unmapped POST
// would fall through to users:write:self. Registered through the P-UC1 seam, so
// the core table cannot be re-scoped by it.
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if method == http.MethodPost && path == "/api/v1/internal/spaces/dm-eligibility" {
			return "spaces:resolve", true
		}
		return "", false
	})
}
