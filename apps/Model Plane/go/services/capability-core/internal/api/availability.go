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

const maxAvailabilityBodyBytes = 16 << 10

var reasonCodePattern = regexp.MustCompile(`^[a-z][a-z0-9_]{0,127}$`)

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
	return principal.PrincipalType == "service" && principal.HasScope(authz.GlobalHealthWriteScope)
}

func (h *CapabilitiesHandler) attestAvailability(w http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodPost {
		http.Error(w, "method not allowed", http.StatusMethodNotAllowed)
		return
	}
	if h.availabilityStore == nil {
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

	principal, principalPresent := authctx.PrincipalFromContext(request.Context())
	organizationID := verifiedOrganizationID(request)
	globalAuthority := principalPresent && mayAttestGlobalCapability(principal)
	var row *registry.CapabilityRow
	var err error
	if globalAuthority {
		row, err = h.availabilityStore.GetGlobal(request.Context(), strings.TrimSpace(input.ID))
	} else {
		row, err = h.availabilityStore.GetForOrg(request.Context(), strings.TrimSpace(input.ID), organizationID)
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
