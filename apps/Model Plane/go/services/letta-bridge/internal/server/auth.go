package server

import (
	"errors"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

var errMemoryUnauthorized = errors.New("memory operation is not authorized")

// MemoryAuthorizer binds every memory operation to the organization in the
// verified JWT. User principals may read and write their tenant's memory;
// workload identities require the corresponding signed scope.
func MemoryAuthorizer(principal authctx.Principal, method string, request any) error {
	if principal.OrganizationID == "" || principal.ActorID == "" {
		return errMemoryUnauthorized
	}
	requestedOrg := ""
	switch value := request.(type) {
	case *mpv1.SearchMemoryRequest:
		requestedOrg = value.GetOrgId()
	case *mpv1.IndexMemoryRequest:
		requestedOrg = value.GetOrgId()
	case *mpv1.MemoryHealthRequest:
		return nil
	default:
		return errMemoryUnauthorized
	}
	if requestedOrg == "" || requestedOrg != principal.OrganizationID {
		return errMemoryUnauthorized
	}
	if !principal.RetentionPolicyPresent || principal.ZeroDataRetention {
		return errMemoryUnauthorized
	}
	if principal.PrincipalType == "user" {
		return nil
	}
	if principal.PrincipalType != "service" {
		return errMemoryUnauthorized
	}
	if method == mpv1.MemoryService_SearchMemory_FullMethodName && principal.HasScope("memory:read") {
		return nil
	}
	if method == mpv1.MemoryService_IndexMemory_FullMethodName && principal.HasScope("memory:write") {
		return nil
	}
	return errMemoryUnauthorized
}
