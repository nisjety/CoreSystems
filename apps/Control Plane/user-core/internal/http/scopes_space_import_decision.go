package http

import (
	"net/http"
	"strings"
)

// WS14-S1: scope of the unified import-decision route, registered through the
// P-UC1 seam. It is an issuer route like personal-import-decision, so it needs
// spaces:issue (a verified gateway delegation proves the end user).
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if method == http.MethodPost && strings.TrimSuffix(path, "/") == "/api/v1/internal/spaces/import-decision" {
			return "spaces:issue", true
		}
		return "", false
	})
}
