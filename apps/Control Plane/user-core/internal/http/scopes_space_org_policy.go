package http

import (
	"net/http"
	"strings"
)

// WS03-S3: scopes of the org-admin Space policy routes, registered through the
// P-UC1 seam. Reads (the view and the history) need spaces:policy:admin:read;
// anything else on the prefix needs spaces:policy:admin:write. Both scopes
// require a verified gateway delegation (a service credential alone never
// proves an org admin). Removing the two scopes from the verevon-gateway
// credential is the hard kill switch.
const spaceOrgPolicyPathPrefix = "/api/v1/internal/spaces/org-policy"

func init() {
	registerSpaceScopeRule(func(method, path string) (string, bool) {
		if path != spaceOrgPolicyPathPrefix && !strings.HasPrefix(path, spaceOrgPolicyPathPrefix+"/") {
			return "", false
		}
		if method == http.MethodGet || method == http.MethodHead {
			return spacePolicyAdminReadScope, true
		}
		return spacePolicyAdminWriteScope, true
	})
	registerSpaceDelegatedScopePrefix("spaces:policy:admin:")
}
