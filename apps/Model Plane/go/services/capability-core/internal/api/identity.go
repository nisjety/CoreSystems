package api

import (
	"net/http"
	"strings"

	"github.com/triodelab/model-plane/pkg/authctx"
)

func verifiedOrganizationID(request *http.Request) string {
	principal, ok := authctx.PrincipalFromContext(request.Context())
	if !ok {
		return ""
	}
	return principal.OrganizationID
}

func verifiedActorID(request *http.Request) string {
	principal, ok := authctx.PrincipalFromContext(request.Context())
	if !ok {
		return ""
	}
	return principal.ActorID
}

// verifiedHasScope reports whether the verified principal attached to
// request carries scope. It never trusts request headers, query parameters,
// or the body — only the signed principal placed in context by Auth Core
// middleware.
func verifiedHasScope(request *http.Request, scope string) bool {
	principal, ok := authctx.PrincipalFromContext(request.Context())
	if !ok {
		return false
	}
	return principal.HasScope(scope)
}

// verifiedIsAdmin derives admin status exclusively from the verified
// principal's signed scopes, mirroring model-gateway's is_admin_claim
// (rust/services/model-gateway/src/ownership.rs) so the two authorities
// agree on what counts as admin.
func verifiedIsAdmin(request *http.Request) bool {
	principal, ok := authctx.PrincipalFromContext(request.Context())
	if !ok {
		return false
	}
	for _, scope := range principal.Scopes {
		if scope == "admin" || scope == "org:admin" || strings.HasSuffix(scope, ":admin") {
			return true
		}
	}
	return false
}
