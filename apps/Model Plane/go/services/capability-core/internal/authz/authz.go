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
	// GlobalHealthWriteScope is reserved for the dedicated workload identity
	// that attests process-wide execution-dispatch capability health. It never
	// grants catalog mutation and tenant health reporters cannot use it.
	GlobalHealthWriteScope = "capability:health:global:write"
	// SpaceDeletionScope is held only by the Control-coordinated deletion
	// workload. It permits the narrow internal cancellation adapter below; it
	// is intentionally not interchangeable with general capability writes.
	SpaceDeletionScope = "capability:space-delete"
	// RiskOverrideScope is required, in addition to WriteScope, to lower the
	// risk_level of a "floored" capability — one whose seed or currently
	// persisted risk_level is high (registry.CapabilitiesStore.Upsert enforces
	// the floor; api.CapabilitiesHandler's upsert handler pre-checks it for
	// statically seeded ids). Kept deliberately separate from WriteScope so an
	// ordinary catalog writer cannot silently disable the human-approval gate
	// on a high-risk capability such as cap.command.shell. See POL-1 in
	// apps/QM_INSPIRED_IMPROVEMENT_PLAN_2026-08-13.md.
	RiskOverrideScope = "capability:risk:override"
)

var errDenied = errors.New("capability-core authorization denied")

// AuthorizeHTTP permits tenant-contained user reads. Service reads require a
// signed capability scope. Every non-read request is treated as a durable
// mutation unless it is explicitly added to the read-only set below: a caller
// must present an explicit, verified non-ZDR posture before any handler can
// reach a database, publisher, or other durable side effect. Request headers,
// query parameters, and bodies cannot weaken that signed posture.
func AuthorizeHTTP(principal authctx.Principal, request *http.Request) error {
	if principal.OrganizationID == "" || principal.ActorID == "" {
		return errDenied
	}
	query := request.URL.Query()
	if conflicts(query.Get("org_id"), principal.OrganizationID) ||
		conflicts(query.Get("user_id"), principal.ActorID) {
		return errDenied
	}
	if !isReadMethod(request.Method) && (!principal.RetentionPolicyPresent || principal.ZeroDataRetention) {
		return errDenied
	}
	if request.URL.Path == "/api/v1/capabilities/availability" {
		if request.Method == http.MethodPost &&
			principal.PrincipalType == "service" &&
			(principal.HasScope(HealthWriteScope) || principal.HasScope(GlobalHealthWriteScope)) {
			return nil
		}
		return errDenied
	}
	if request.URL.Path == "/api/v1/internal/space-deletion/cron" {
		if request.Method == http.MethodPost && principal.PrincipalType == "service" && principal.HasScope(SpaceDeletionScope) {
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
		// Promotion changes the shared registry scope. Its caller has no body
		// field that may weaken the signed retention posture, so reject both
		// issuer-ZDR and omitted posture before this mutating RPC can run.
		if !principal.RetentionPolicyPresent || principal.ZeroDataRetention {
			return errDenied
		}
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
