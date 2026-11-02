package http

import (
	"net/http"
	"strings"
)

// WS12-S5: scopes of the message-lifecycle routes, registered through the P-UC1
// seam. Without these rules an unmapped POST or PUT would fall through to
// users:write:self.
//
//	POST /api/v1/internal/spaces/thread-revise-decision    -> spaces:issue (delegated)
//	GET  /api/v1/internal/spaces/{ref}/message-policy      -> spaces:resolve (delegated)
//	PUT  /api/v1/internal/spaces/message-policy            -> spaces:policy:write
func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok {
			return "", false
		}
		parts := strings.Split(rest, "/")
		switch {
		case len(parts) == 1 && parts[0] == "thread-revise-decision" && method == http.MethodPost:
			return "spaces:issue", true
		case len(parts) == 1 && parts[0] == "message-policy" && method == http.MethodPut:
			return "spaces:policy:write", true
		case len(parts) == 2 && parts[1] == "message-policy" && method == http.MethodGet:
			return "spaces:resolve", true
		}
		return "", false
	})
}
