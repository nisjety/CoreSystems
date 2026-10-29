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
	// Auth Core namespaces workload identities in the signed service_id/sub
	// claim. Downstream gates must compare this exact value rather than the
	// deployment-name suffix, which would accept an ambiguous principal.
	ExecutionCoreServiceID    = "service:execution-core"
	ConversationCoreServiceID = "service:conversation-core"
	BrowserBrokerServiceID    = "service:browser-broker"

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
	// ModelActionViewScope permits Execution Core to exchange a fresh,
	// Control-signed run view for a server-owned model tool definition. It is
	// presentation-only and is never an owner-effect or catalog-write scope.
	ModelActionViewScope = "capability:model-action:view"
	// OwnerActionHealthWriteScope is held only by Conversation Core's dedicated
	// owner-action readiness reporter. It cannot mutate the normal catalog or
	// attest any capability other than the ticket adapter.
	OwnerActionHealthWriteScope = "capability:owner-action:health:write"
	// BrowserHealthWriteScope is held only by browser-broker's readiness
	// reporter. It cannot write the catalog or attest any capability other
	// than cap.browser.open, whose runtime (grants and the Quarry validation
	// path) the broker owns.
	BrowserHealthWriteScope = "capability:browser:health:write"
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
			((principal.HasScope(GlobalHealthWriteScope) && principal.ActorID == ExecutionCoreServiceID && principal.OrganizationID == "global") ||
				(principal.HasScope(HealthWriteScope) && principal.OrganizationID != "global")) {
			return nil
		}
		return errDenied
	}
	if request.URL.Path == "/api/v1/internal/model-actions/run-view" {
		if request.Method == http.MethodPost && principal.PrincipalType == "service" &&
			principal.ActorID == ExecutionCoreServiceID && principal.HasScope(ModelActionViewScope) {
			return nil
		}
		return errDenied
	}
	if request.URL.Path == "/api/v1/capabilities/owner-actions/health" {
		if request.Method == http.MethodPost && principal.PrincipalType == "service" &&
			principal.ActorID == ConversationCoreServiceID && principal.OrganizationID == "global" &&
			principal.HasScope(OwnerActionHealthWriteScope) {
			return nil
		}
		return errDenied
	}
	if request.URL.Path == "/api/v1/capabilities/browser/health" {
		if request.Method == http.MethodPost && principal.PrincipalType == "service" &&
			principal.ActorID == BrowserBrokerServiceID && principal.OrganizationID == "global" &&
			principal.HasScope(BrowserHealthWriteScope) {
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
// EvaluatePolicy or ResolveToolBindings tenant that conflicts with the signed
// tenant.
//
// A user_id on those two requests names the person a run acts for. It is
// asserted, not proven, by the caller, so only the execution-core workload may
// assert one (the recorded trust assumption: execution-core takes it from the
// run it was started for), and a person may only name themselves.
func AuthorizeGRPC(principal authctx.Principal, method string, request any) error {
	if principal.OrganizationID == "" || principal.ActorID == "" {
		return errDenied
	}
	if evaluate, ok := request.(*mpv1.EvaluatePolicyRequest); ok &&
		conflicts(evaluate.GetOrgId(), principal.OrganizationID) {
		return errDenied
	}
	if resolve, ok := request.(*mpv1.ResolveToolBindingsRequest); ok &&
		conflicts(resolve.GetOrgId(), principal.OrganizationID) {
		return errDenied
	}
	if method == mpv1.CapabilityCore_EvaluatePolicy_FullMethodName {
		evaluate, _ := request.(*mpv1.EvaluatePolicyRequest)
		if evaluate.GetUserId() != "" && principal.ActorID != ExecutionCoreServiceID {
			return errDenied
		}
		if principal.PrincipalType == "service" &&
			(principal.HasScope(ReadScope) || principal.HasScope(WriteScope) || principal.HasScope(GlobalWriteScope)) {
			return nil
		}
		return errDenied
	}
	if method == mpv1.CapabilityCore_ResolveToolBindings_FullMethodName {
		resolve, _ := request.(*mpv1.ResolveToolBindingsRequest)
		switch principal.PrincipalType {
		case "service":
			if resolve.GetUserId() != "" && principal.ActorID != ExecutionCoreServiceID {
				return errDenied
			}
			if principal.HasScope(ReadScope) || principal.HasScope(WriteScope) {
				return nil
			}
		case "user":
			if resolve.GetUserId() == "" || resolve.GetUserId() == principal.ActorID {
				return nil
			}
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
