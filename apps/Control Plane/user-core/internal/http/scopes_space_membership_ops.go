package http

import (
	"net/http"
	"strings"
)

// WS04-S4: scopes of the room membership writer, registered through the P-UC1
// seam (feature rules run only after the core space table did not match, so
// they cannot re-scope an existing route).
//
//	any method on /api/v1/internal/spaces/{ref}/members[/{id}] -> spaces:membership:act
//	GET /api/v1/internal/spaces/{ref}/audience-candidates      -> spaces:membership:read
//
// spaces:membership:act is registered as a delegated scope: a service
// credential alone never proves the acting user. spaces:membership:read is a
// plain service scope. Without the rule an unmapped POST would fall through
// to users:write:self; every ACT handler guard also asserts the scope itself.
func init() {
	registerSpaceDelegatedScopePrefix(spaceMembershipActScope)
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		rest, ok := strings.CutPrefix(path, "/api/v1/internal/spaces/")
		if !ok {
			return "", false
		}
		parts := strings.Split(rest, "/")
		switch {
		case len(parts) == 2 && parts[1] == "members",
			len(parts) == 3 && parts[1] == "members":
			return spaceMembershipActScope, true
		case len(parts) == 2 && parts[1] == "audience-candidates" && method == http.MethodGet:
			return spaceMembershipReadScope, true
		}
		return "", false
	})
}
