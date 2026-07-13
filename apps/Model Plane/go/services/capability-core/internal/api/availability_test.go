package api

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

type fakeAvailabilityStore struct {
	row         *registry.CapabilityRow
	getErr      error
	updateErr   error
	updated     bool
	gotOrgID    string
	gotUpdate   registry.AvailabilityUpdate
	auditAction string
}

func (store *fakeAvailabilityStore) GetForOrg(_ context.Context, _ string, organizationID string) (*registry.CapabilityRow, error) {
	store.gotOrgID = organizationID
	if store.getErr != nil {
		return nil, store.getErr
	}
	return store.row, nil
}

func (store *fakeAvailabilityStore) AttestAvailabilityForOrg(_ context.Context, _, organizationID, _ string, update registry.AvailabilityUpdate) (bool, error) {
	store.gotOrgID = organizationID
	store.gotUpdate = update
	store.auditAction = "availability_attested"
	return store.updated, store.updateErr
}

func availabilityRecorder(t *testing.T, method, body string, store *fakeAvailabilityStore) *httptest.ResponseRecorder {
	t.Helper()
	handler := &CapabilitiesHandler{availabilityStore: store}
	request := httptest.NewRequest(method, "/api/v1/capabilities/availability", strings.NewReader(body))
	recorder := httptest.NewRecorder()
	handler.attestAvailability(recorder, request)
	return recorder
}

func TestAttestAvailabilityAcceptsBoundedTenantUpdate(t *testing.T) {
	t.Parallel()

	store := &fakeAvailabilityStore{
		row:     &registry.CapabilityRow{ID: "cap.read", Version: "1", Enabled: true, RiskLevel: models.RiskLow},
		updated: true,
	}
	recorder := availabilityRecorder(t, http.MethodPost, `{
		"id":"cap.read","version":"1","state":"available","reason_code":"runtime_healthy",
		"reason":"probe succeeded","execution_mode":"direct_read","cost_class":"bounded"
	}`, store)

	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.gotUpdate.ExpectedVersion != "1" || store.gotUpdate.State != string(models.AvailabilityAvailable) || store.gotUpdate.HealthCheckedAt.IsZero() {
		t.Fatalf("update = %+v", store.gotUpdate)
	}
	if store.gotUpdate.Reason != "Capability health check succeeded." {
		t.Fatalf("reporter-controlled reason reached the public contract: %q", store.gotUpdate.Reason)
	}
	if store.auditAction != "availability_attested" {
		t.Fatalf("audit action = %q", store.auditAction)
	}
	var response struct {
		Data models.Availability `json:"data"`
	}
	if err := json.NewDecoder(recorder.Body).Decode(&response); err != nil {
		t.Fatal(err)
	}
	if response.Data.State != models.AvailabilityAvailable || response.Data.ExecutionMode != models.ExecutionDirectRead {
		t.Fatalf("response = %+v", response.Data)
	}
}

func TestAttestAvailabilityFailsClosedAtEveryBoundary(t *testing.T) {
	t.Parallel()

	validRow := &registry.CapabilityRow{ID: "cap.read", Version: "1", Enabled: true, RiskLevel: models.RiskLow}
	validBody := `{"id":"cap.read","version":"1","state":"available","reason_code":"runtime_healthy","execution_mode":"direct_read"}`
	tests := []struct {
		name       string
		method     string
		body       string
		store      *fakeAvailabilityStore
		wantStatus int
	}{
		{name: "method", method: http.MethodGet, store: &fakeAvailabilityStore{}, wantStatus: http.StatusMethodNotAllowed},
		{name: "malformed", method: http.MethodPost, body: `{`, store: &fakeAvailabilityStore{}, wantStatus: http.StatusBadRequest},
		{name: "unknown field", method: http.MethodPost, body: `{"id":"cap.read","unknown":true}`, store: &fakeAvailabilityStore{}, wantStatus: http.StatusBadRequest},
		{name: "multiple values", method: http.MethodPost, body: validBody + `{}`, store: &fakeAvailabilityStore{}, wantStatus: http.StatusBadRequest},
		{name: "oversized", method: http.MethodPost, body: `{"id":"cap.read","reason":"` + strings.Repeat("x", maxAvailabilityBodyBytes) + `"}`, store: &fakeAvailabilityStore{}, wantStatus: http.StatusBadRequest},
		{name: "missing", method: http.MethodPost, body: validBody, store: &fakeAvailabilityStore{getErr: errors.New("not found")}, wantStatus: http.StatusNotFound},
		{name: "tenant mismatch", method: http.MethodPost, body: validBody, store: &fakeAvailabilityStore{row: &registry.CapabilityRow{ID: "cap.read", OrgID: "another-org"}}, wantStatus: http.StatusNotFound},
		{name: "invalid attestation", method: http.MethodPost, body: `{"id":"cap.read","state":"green","reason_code":"runtime_healthy"}`, store: &fakeAvailabilityStore{row: validRow}, wantStatus: http.StatusUnprocessableEntity},
		{name: "update error", method: http.MethodPost, body: validBody, store: &fakeAvailabilityStore{row: validRow, updateErr: errors.New("database unavailable")}, wantStatus: http.StatusInternalServerError},
		{name: "concurrent delete", method: http.MethodPost, body: validBody, store: &fakeAvailabilityStore{row: validRow}, wantStatus: http.StatusNotFound},
	}

	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			recorder := availabilityRecorder(t, test.method, test.body, test.store)
			if recorder.Code != test.wantStatus {
				t.Fatalf("status = %d, want %d, body = %s", recorder.Code, test.wantStatus, recorder.Body.String())
			}
			if bytes.Contains(recorder.Body.Bytes(), []byte("database unavailable")) {
				t.Fatalf("response leaked internal error: %s", recorder.Body.String())
			}
		})
	}
}

func TestNormalizeAvailabilityAttestationRequiresBoundedValidInput(t *testing.T) {
	t.Parallel()

	now := time.Date(2026, time.July, 13, 14, 0, 0, 0, time.UTC)
	row := &registry.CapabilityRow{ID: "cap.tool.read", Version: "1", Enabled: true, RiskLevel: models.RiskLow}

	tests := []struct {
		name    string
		request availabilityRequest
		wantErr bool
	}{
		{name: "valid", request: availabilityRequest{ID: row.ID, Version: row.Version, State: "available", ReasonCode: "runtime_healthy", ExecutionMode: "direct_read", CostClass: "bounded"}},
		{name: "missing id", request: availabilityRequest{State: "available", ReasonCode: "runtime_healthy", ExecutionMode: "direct_read"}, wantErr: true},
		{name: "wrong version", request: availabilityRequest{ID: row.ID, Version: "2", State: "available", ReasonCode: "runtime_healthy", ExecutionMode: "direct_read"}, wantErr: true},
		{name: "invalid state", request: availabilityRequest{ID: row.ID, State: "green", ReasonCode: "runtime_healthy", ExecutionMode: "direct_read"}, wantErr: true},
		{name: "invalid reason token", request: availabilityRequest{ID: row.ID, State: "available", ReasonCode: "Contains spaces", ExecutionMode: "direct_read"}, wantErr: true},
		{name: "available without execution", request: availabilityRequest{ID: row.ID, State: "available", ReasonCode: "runtime_healthy"}, wantErr: true},
	}

	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			_, err := normalizeAvailabilityAttestation(row, test.request, now)
			if (err != nil) != test.wantErr {
				t.Fatalf("error = %v, wantErr %v", err, test.wantErr)
			}
		})
	}
}

func TestNormalizeAvailabilityAttestationCannotBypassApprovalOrDisabledPolicy(t *testing.T) {
	t.Parallel()

	now := time.Date(2026, time.July, 13, 14, 0, 0, 0, time.UTC)
	risky := &registry.CapabilityRow{ID: "cap.sandbox.exec", Version: "1", Enabled: true, RiskLevel: models.RiskHigh}
	attestation := availabilityRequest{ID: risky.ID, Version: risky.Version, State: "available", ReasonCode: "runtime_healthy", ExecutionMode: "agentic", CostClass: "variable"}

	got, err := normalizeAvailabilityAttestation(risky, attestation, now)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got.State != models.AvailabilityApprovalRequired || !got.RequiresApproval {
		t.Fatalf("risky availability = %+v", got)
	}

	disabled := *risky
	disabled.Enabled = false
	got, err = normalizeAvailabilityAttestation(&disabled, attestation, now)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if got.State != models.AvailabilityDisabled || got.ExecutionMode != models.ExecutionUnavailable {
		t.Fatalf("disabled availability = %+v", got)
	}
}

func TestCapabilityWireViewNeverExposesRawOrStaleAvailability(t *testing.T) {
	t.Parallel()
	if capabilityWireView(nil) != nil {
		t.Fatal("nil durable capability must remain nil")
	}

	stale := time.Now().UTC().Add(-models.AvailabilityAttestationTTL - time.Minute)
	view := capabilityWireView(&registry.CapabilityRow{
		ID:                "cap.read",
		Enabled:           true,
		RiskLevel:         models.RiskLow,
		AvailabilityState: "available",
		ReasonCode:        "runtime_healthy",
		ExecutionMode:     "direct_read",
		HealthCheckedAt:   &stale,
		ConfigJSON:        []byte(`{"credential":"must-not-leak"}`),
	})

	if view["state"] != models.AvailabilityUnavailable || view["reason_code"] != "health_attestation_stale" {
		t.Fatalf("wire availability = %#v", view)
	}
	if _, exposed := view["config_json"]; exposed {
		t.Fatal("wire capability must not expose runtime configuration")
	}

	current := time.Now().UTC()
	quarantined := capabilityWireView(&registry.CapabilityRow{
		ID: "cap.quarantined", Enabled: true, RiskLevel: models.RiskLow,
		RolloutState: "quarantine", AvailabilityState: "available",
		ExecutionMode: "direct_read", HealthCheckedAt: &current,
	})
	if quarantined["state"] != models.AvailabilityUnavailable || quarantined["reason_code"] != "rollout_quarantine" {
		t.Fatalf("quarantined wire availability = %#v", quarantined)
	}
}

func TestPublicAvailabilityReasonsAreServerControlledForEveryState(t *testing.T) {
	t.Parallel()

	tests := []struct {
		state string
		want  string
	}{
		{state: string(models.AvailabilityAvailable), want: "Capability health check succeeded."},
		{state: string(models.AvailabilityDisabled), want: "Capability is disabled by policy."},
		{state: string(models.AvailabilityUnhealthy), want: "Capability runtime is unhealthy."},
		{state: string(models.AvailabilityApprovalRequired), want: "Capability requires governed approval."},
		{state: string(models.AvailabilityNotConfigured), want: "Capability runtime is not configured."},
		{state: string(models.AvailabilityUnavailable), want: "Capability runtime is unavailable."},
	}

	for _, test := range tests {
		test := test
		t.Run(test.state, func(t *testing.T) {
			t.Parallel()
			if got := publicAvailabilityReason(test.state); got != test.want {
				t.Fatalf("reason = %q, want %q", got, test.want)
			}
		})
	}
}
