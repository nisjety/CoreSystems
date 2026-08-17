package api

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"regexp"
	"strings"
	"time"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

const (
	maxAvailabilityBodyBytes = 16 << 10

	// ownerActionTicketCapabilityID is intentionally excluded from the generic
	// execution-runtime health path. A sandbox probe can truthfully attest only
	// execution-core's own command capabilities; it cannot establish the live
	// Control, Conversation Core, approval, and run-bound authority predicates
	// required before an owner-action ticket may be offered or executed. A
	// future owner-action attester must have its own contract and route instead
	// of widening this credential.
	ownerActionTicketCapabilityID = "cap.tool.ticket.create"

	// genericGlobalHealthAttesterID is the only workload currently permitted to
	// use the generic global-health route. Its Rust reporter probes precisely
	// the two capabilities listed below; a scope alone is intentionally not a
	// universal availability authority.
	// Auth Core's service-token contract prefixes service identities in both
	// `sub` and `service_id`; keep the exact signed namespace in this comparison.
	genericGlobalHealthAttesterID = authz.ExecutionCoreServiceID
)

var reasonCodePattern = regexp.MustCompile(`^[a-z][a-z0-9_]{0,127}$`)

var genericGlobalHealthCapabilityIDs = map[string]struct{}{
	"cap.command.sandbox": {},
	"cap.command.shell":   {},
}

type availabilityRequest struct {
	ID            string `json:"id"`
	Version       string `json:"version"`
	State         string `json:"state"`
	ReasonCode    string `json:"reason_code"`
	Reason        string `json:"reason"`
	ExecutionMode string `json:"execution_mode"`
	CostClass     string `json:"cost_class"`
}

type normalizedAvailability struct {
	models.Availability
	checkedAt time.Time
}

func capabilityWireView(row *registry.CapabilityRow) map[string]any {
	if row == nil {
		return nil
	}
	availability := models.DeriveAvailability(&models.Capability{
		ID:                row.ID,
		Enabled:           row.Enabled,
		RiskLevel:         row.RiskLevel,
		RolloutState:      row.RolloutState,
		AvailabilityState: row.AvailabilityState,
		ReasonCode:        row.ReasonCode,
		Reason:            row.Reason,
		ExecutionMode:     row.ExecutionMode,
		CostClass:         row.CostClass,
		HealthCheckedAt:   row.HealthCheckedAt,
	})
	return map[string]any{
		"id":                 row.ID,
		"org_id":             row.OrgID,
		"kind":               row.Kind,
		"name":               row.Name,
		"version":            row.Version,
		"description":        row.Description,
		"risk_level":         row.RiskLevel,
		"scope":              row.Scope,
		"lazy_load":          row.LazyLoad,
		"enabled":            row.Enabled,
		"rollout_state":      row.RolloutState,
		"tags":               append([]string(nil), row.Tags...),
		"enabled_for_scopes": append([]string(nil), row.EnabledForScopes...),
		"state":              availability.State,
		"reason_code":        availability.ReasonCode,
		"reason":             availability.Reason,
		"requires_approval":  availability.RequiresApproval,
		"execution_mode":     availability.ExecutionMode,
		"cost_class":         availability.CostClass,
		"health_checked_at":  availability.HealthCheckedAt,
	}
}

type availabilityStoreBackend interface {
	GetForOrg(context.Context, string, string) (*registry.CapabilityRow, error)
	GetGlobal(context.Context, string) (*registry.CapabilityRow, error)
	AttestAvailabilityForOrg(context.Context, string, string, string, registry.AvailabilityUpdate) (bool, error)
	AttestAvailabilityGlobal(context.Context, string, string, registry.AvailabilityUpdate) (bool, error)
}

func mayAttestGlobalCapability(principal authctx.Principal) bool {
	// Keep this invariant at the handler boundary as well as in the public
	// HTTP authorizer. A miswired route or an internal caller must not turn a
	// global-health scope into cross-tenant authority: only the exact
	// execution-core workload, with the reserved global organization binding,
	// may select GetGlobal/AttestAvailabilityGlobal.
	return principal.PrincipalType == "service" &&
		principal.OrganizationID == "global" &&
		principal.ActorID == authz.ExecutionCoreServiceID &&
		principal.HasScope(authz.GlobalHealthWriteScope)
}

// mayUseGenericGlobalHealthAttestation narrows the generic health credential
// to capabilities whose runtime it can actually measure. It deliberately
// remains separate from mayAttestGlobalCapability: that scope chooses the
// global registry lane, while this predicate prevents it from becoming a
// universal "make available" authority.
func mayUseGenericGlobalHealthAttestation(principal authctx.Principal, capabilityID string) bool {
	if !mayAttestGlobalCapability(principal) ||
		principal.ActorID != genericGlobalHealthAttesterID {
		return false
	}
	_, allowed := genericGlobalHealthCapabilityIDs[strings.TrimSpace(capabilityID)]
	return allowed
}

func (h *CapabilitiesHandler) attestAvailability(w http.ResponseWriter, request *http.Request) {
	h.attestAvailabilityForLane(w, request, false)
}

// attestOwnerActionHealth is intentionally a different route and service
// identity from generic runtime health. Conversation Core can only attest its
// ticket adapter and only after Capability Core itself has a Control public-key
// verifier; it cannot make arbitrary Model capabilities runnable.
func (h *CapabilitiesHandler) attestOwnerActionHealth(w http.ResponseWriter, request *http.Request) {
	h.attestAvailabilityForLane(w, request, true)
}

func (h *CapabilitiesHandler) attestAvailabilityForLane(w http.ResponseWriter, request *http.Request, ownerActionLane bool) {
	if request.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if h.availabilityStore == nil || (ownerActionLane && h.modelActionViews == nil) {
		jsonErr(w, "availability update failed", http.StatusServiceUnavailable)
		return
	}

	request.Body = http.MaxBytesReader(w, request.Body, maxAvailabilityBodyBytes)
	decoder := json.NewDecoder(request.Body)
	decoder.DisallowUnknownFields()
	var input availabilityRequest
	if err := decoder.Decode(&input); err != nil {
		jsonErr(w, "invalid availability attestation", http.StatusBadRequest)
		return
	}
	if err := ensureJSONEOF(decoder); err != nil {
		jsonErr(w, "invalid availability attestation", http.StatusBadRequest)
		return
	}

	capabilityID := strings.TrimSpace(input.ID)
	principal, principalPresent := authctx.PrincipalFromContext(request.Context())
	organizationID := verifiedOrganizationID(request)
	globalAuthority := principalPresent && mayAttestGlobalCapability(principal)
	if ownerActionLane {
		if !principalPresent || principal.PrincipalType != "service" ||
			principal.ActorID != authz.ConversationCoreServiceID ||
			!principal.HasScope(authz.OwnerActionHealthWriteScope) ||
			organizationID != "global" || capabilityID != ownerActionTicketCapabilityID {
			jsonErr(w, "owner-action health attestation is not authorized", http.StatusForbidden)
			return
		}
		globalAuthority = true
	}
	// Owner actions are never eligible for the generic health route, including
	// its tenant lane. This independently backs up the HTTP authorizer: a
	// future route-policy regression cannot let a tenant health reporter make a
	// globally registered owner action runnable.
	if !ownerActionLane && capabilityID == ownerActionTicketCapabilityID {
		jsonErr(w, "owner-action capability requires a dedicated health attester", http.StatusForbidden)
		return
	}
	if !ownerActionLane && !globalAuthority && organizationID == "global" {
		jsonErr(w, "tenant health attester may not target global capability health", http.StatusForbidden)
		return
	}
	if !ownerActionLane && globalAuthority && !mayUseGenericGlobalHealthAttestation(principal, capabilityID) {
		jsonErr(w, "generic global health attester may not attest this capability", http.StatusForbidden)
		return
	}
	var row *registry.CapabilityRow
	var err error
	if globalAuthority {
		row, err = h.availabilityStore.GetGlobal(request.Context(), capabilityID)
	} else {
		row, err = h.availabilityStore.GetForOrg(request.Context(), capabilityID, organizationID)
	}
	if err != nil || row == nil || (!globalAuthority && row.OrgID != organizationID) || (globalAuthority && row.OrgID != "global") {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}

	now := time.Now().UTC()
	normalized, err := normalizeAvailabilityAttestation(row, input, now)
	if err != nil {
		jsonErr(w, "invalid availability attestation", http.StatusUnprocessableEntity)
		return
	}
	actor := verifiedActorID(request)
	update := registry.AvailabilityUpdate{
		State:           string(normalized.State),
		ExpectedVersion: row.Version,
		ReasonCode:      normalized.ReasonCode,
		Reason:          normalized.Reason,
		ExecutionMode:   normalized.ExecutionMode,
		CostClass:       normalized.CostClass,
		HealthCheckedAt: normalized.checkedAt,
	}
	var updated bool
	if globalAuthority {
		updated, err = h.availabilityStore.AttestAvailabilityGlobal(request.Context(), row.ID, actor, update)
	} else {
		updated, err = h.availabilityStore.AttestAvailabilityForOrg(request.Context(), row.ID, organizationID, actor, update)
	}
	if err != nil {
		jsonErr(w, "availability update failed", http.StatusInternalServerError)
		return
	}
	if !updated {
		jsonErr(w, "not found", http.StatusNotFound)
		return
	}

	writeJSON(w, map[string]any{"data": map[string]any{
		"id":                row.ID,
		"state":             normalized.State,
		"reason_code":       normalized.ReasonCode,
		"reason":            normalized.Reason,
		"requires_approval": normalized.RequiresApproval,
		"execution_mode":    normalized.ExecutionMode,
		"cost_class":        normalized.CostClass,
		"health_checked_at": normalized.HealthCheckedAt,
	}})
}

func normalizeAvailabilityAttestation(row *registry.CapabilityRow, input availabilityRequest, checkedAt time.Time) (normalizedAvailability, error) {
	input.ID = strings.TrimSpace(input.ID)
	input.Version = strings.TrimSpace(input.Version)
	input.State = strings.TrimSpace(input.State)
	input.ReasonCode = strings.TrimSpace(input.ReasonCode)
	input.Reason = strings.TrimSpace(input.Reason)
	input.ExecutionMode = strings.TrimSpace(input.ExecutionMode)
	input.CostClass = strings.TrimSpace(input.CostClass)

	if row == nil || input.ID == "" || input.ID != row.ID || len(input.ID) > 200 {
		return normalizedAvailability{}, errors.New("invalid capability id")
	}
	if input.Version == "" || input.Version != row.Version || len(input.Version) > 100 {
		return normalizedAvailability{}, errors.New("invalid capability version")
	}
	if !validAvailabilityState(input.State) || !reasonCodePattern.MatchString(input.ReasonCode) {
		return normalizedAvailability{}, errors.New("invalid availability state or reason")
	}
	if len(input.Reason) > 500 || !validExecutionMode(input.ExecutionMode) || !validCostClass(input.CostClass) {
		return normalizedAvailability{}, errors.New("invalid availability detail")
	}
	if input.State == string(models.AvailabilityAvailable) && input.ExecutionMode == "" {
		return normalizedAvailability{}, errors.New("available capability requires execution mode")
	}

	healthCheckedAt := checkedAt.UTC()
	availability := models.DeriveAvailabilityAt(&models.Capability{
		ID:                row.ID,
		Enabled:           row.Enabled,
		RiskLevel:         row.RiskLevel,
		RolloutState:      row.RolloutState,
		AvailabilityState: input.State,
		ReasonCode:        input.ReasonCode,
		Reason:            publicAvailabilityReason(input.State),
		ExecutionMode:     input.ExecutionMode,
		CostClass:         input.CostClass,
		HealthCheckedAt:   &healthCheckedAt,
	}, healthCheckedAt)
	if availability.ReasonCode == "invalid_availability_attestation" ||
		availability.ReasonCode == "execution_mode_unavailable" {
		return normalizedAvailability{}, errors.New("invalid availability attestation")
	}
	return normalizedAvailability{Availability: availability, checkedAt: healthCheckedAt}, nil
}

func publicAvailabilityReason(state string) string {
	switch models.AvailabilityState(state) {
	case models.AvailabilityAvailable:
		return "Capability health check succeeded."
	case models.AvailabilityDisabled:
		return "Capability is disabled by policy."
	case models.AvailabilityUnhealthy:
		return "Capability runtime is unhealthy."
	case models.AvailabilityApprovalRequired:
		return "Capability requires governed approval."
	case models.AvailabilityNotConfigured:
		return "Capability runtime is not configured."
	default:
		return "Capability runtime is unavailable."
	}
}

func validAvailabilityState(state string) bool {
	switch models.AvailabilityState(state) {
	case models.AvailabilityAvailable,
		models.AvailabilityDisabled,
		models.AvailabilityUnhealthy,
		models.AvailabilityApprovalRequired,
		models.AvailabilityUnavailable,
		models.AvailabilityNotConfigured:
		return true
	default:
		return false
	}
}

func validExecutionMode(mode string) bool {
	return mode == "" || mode == models.ExecutionDirectRead ||
		mode == models.ExecutionAgentic || mode == models.ExecutionUnavailable
}

func validCostClass(costClass string) bool {
	return costClass == "" || costClass == models.CostUnknown ||
		costClass == models.CostBounded || costClass == models.CostVariable
}

func ensureJSONEOF(decoder *json.Decoder) error {
	var extra any
	if err := decoder.Decode(&extra); !errors.Is(err, io.EOF) {
		if err == nil {
			return errors.New("multiple JSON values")
		}
		return err
	}
	return nil
}
