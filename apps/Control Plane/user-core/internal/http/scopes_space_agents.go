package http

import (
	"net/http"
	"strings"
)

// WS17-S4: scopes of the agent routes. The registry scope agents:manage is served by
// serviceScopeForRequest in service_auth.go for /api/v1/internal/agents/**. The room
// scope spaces:agents:manage is a feature rule of the P-UC1 seam, which runs only when
// the core space table did not match:
//
//	POST   /api/v1/internal/spaces/{ref}/agent-candidates           -> spaces:agents:manage
//	POST   /api/v1/internal/spaces/{ref}/agent-admissions           -> spaces:agents:manage
//	DELETE /api/v1/internal/spaces/{ref}/agent-admissions/{id}      -> spaces:agents:manage
//
// Both scopes are delegated: a service credential alone never proves the acting user.
func init() {
	registerSpaceDelegatedScopePrefix(agentsManageScope)
	registerSpaceDelegatedScopePrefix(spaceAgentsManageScope)
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok {
			return "", false
		}
		parts := strings.Split(rest, "/")
		switch {
		case len(parts) == 2 && parts[1] == "agent-candidates" && method == http.MethodPost,
			len(parts) == 2 && parts[1] == "agent-admissions" && method == http.MethodPost,
			len(parts) == 3 && parts[1] == "agent-admissions" && method == http.MethodDelete:
			return spaceAgentsManageScope, true
		}
		return "", false
	})
}
