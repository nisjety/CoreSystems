// Package authz defines capability-core authorization policy on top of an
// identity already verified by Auth Core. It never derives authority from
// request headers, query parameters, or request bodies.
package authz

import (
	"errors"
	"net/http"
	"strings"

	mpv1 "github.com/triodelab/model-plane/gen/go/model_plane/v1"
	"github.com/triodelab/model-plane/pkg/authctx"
)

const (
	ReadScope  = "capability:read"
	WriteScope = "capability:write"
	// GlobalWriteScope is reserved for workload identities that maintain the
	// process-wide static skill registry. Tenant writers cannot receive it.
	GlobalWriteScope = "capability:global:write"
	// HealthWriteScope is reserved for workload identities that attest runtime
	// capability health. Ordinary catalog writers and users cannot mint
	// availability.
	HealthWriteScope = "capability:health:write"
)

var errDenied = errors.New("capability-core authorization denied")

// AuthorizeHTTP permits tenant-contained user reads. Service reads require a
// signed capability scope and every mutation requires capability:write.
func AuthorizeHTTP(principal authctx.Principal, request *http.Request) error {
	if principal.OrganizationID == "" || principal.ActorID == "" {
		return errDenied
	}
	query := request.URL.Query()
	if conflicts(query.Get("org_id"), principal.OrganizationID) ||
		conflicts(query.Get("user_id"), principal.ActorID) {
		return errDenied
	}
	if request.URL.Path == "/api/v1/capabilities/availability" {
		if request.Method == http.MethodPost &&
			principal.PrincipalType == "service" &&
			principal.HasScope(HealthWriteScope) {
			return nil
		}
		return errDenied
	}
	if isReadMethod(request.Method) {
		if principal.PrincipalType == "user" || principal.HasScope(ReadScope) || principal.HasScope(WriteScope) {
			return nil
		}
		return errDenied
	}
	// Command execution delegates to session/inference RPCs whose current
	// contracts carry neither the verified tenant nor a downstream bearer.
	// Keep the catalog readable but quarantine execution until those contracts
	// are authenticated and tenant-aware.
	if request.URL.Path == "/api/v1/commands/exec" {
		return errDenied
	}
	if principal.HasScope(WriteScope) {
		return nil
	}
	return errDenied
}

// AuthorizeGRPC applies the same read/write policy and rejects a caller-supplied
// EvaluatePolicy tenant that conflicts with the signed tenant.
func AuthorizeGRPC(principal authctx.Principal, method string, request any) error {
	if principal.OrganizationID == "" || principal.ActorID == "" {
		return errDenied
	}
	if evaluate, ok := request.(*mpv1.EvaluatePolicyRequest); ok &&
		conflicts(evaluate.GetOrgId(), principal.OrganizationID) {
		return errDenied
	}
	if method == mpv1.CapabilityCore_EvaluatePolicy_FullMethodName {
		if principal.PrincipalType == "service" &&
			(principal.HasScope(ReadScope) || principal.HasScope(WriteScope) || principal.HasScope(GlobalWriteScope)) {
			return nil
		}
		return errDenied
	}
	if method == mpv1.CapabilityCore_PromoteSkill_FullMethodName {
		if principal.PrincipalType == "service" && principal.HasScope(GlobalWriteScope) {
			return nil
		}
		return errDenied
	}
	if principal.PrincipalType == "user" || principal.HasScope(ReadScope) || principal.HasScope(WriteScope) {
		return nil
	}
	return errDenied
}

func isReadMethod(method string) bool {
	return method == http.MethodGet || method == http.MethodHead || method == http.MethodOptions
}

func conflicts(untrusted, verified string) bool {
	untrusted = strings.TrimSpace(untrusted)
	return untrusted != "" && untrusted != verified
}
