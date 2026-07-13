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
