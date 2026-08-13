package api

import (
	"net/http"

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
