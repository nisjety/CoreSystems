package api

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// browser-broker owns cap.browser.open: it alone attests that capability's
// runtime health, on its own route and scope, and every other lane refuses the
// id. These tests drive the real Auth Core JWT verifier, the real HTTP
// authorizer and the real route table in front of the handler.

const browserHealthPath = "/api/v1/capabilities/browser/health"

type signedAttestation struct {
	path           string
	organizationID string
	serviceID      string
	scope          string
	zdr            bool
	body           string
}

func browserOpenRow(organizationID string) *registry.CapabilityRow {
	return &registry.CapabilityRow{
		ID: browserOwnerCapabilityID, OrgID: organizationID, Version: "1",
		Enabled: true, RiskLevel: models.RiskHigh, CostClass: models.CostVariable,
	}
}

func browserHealthBody(capabilityID, executionMode string) string {
	return `{"id":"` + capabilityID + `","version":"1","state":"available","reason_code":"browser_broker_ready",` +
		`"reason":"","execution_mode":"` + executionMode + `","cost_class":"variable"}`
}

func browserBrokerAttestation() signedAttestation {
	return signedAttestation{
		path:           browserHealthPath,
		organizationID: "global",
		serviceID:      authz.BrowserBrokerServiceID,
		scope:          authz.BrowserHealthWriteScope,
		body:           browserHealthBody(browserOwnerCapabilityID, models.ExecutionAgentic),
	}
}

// signedAttestationRecorder serves one request through the verifier, the
// authorizer and the full route table, so the route registration is covered too.
func signedAttestationRecorder(t *testing.T, store *fakeAvailabilityStore, input signedAttestation) *httptest.ResponseRecorder {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicKey := pem.EncodeToMemory(&pem.Block{Type: "RSA PUBLIC KEY", Bytes: x509.MarshalPKCS1PublicKey(&key.PublicKey)})
	verifier, err := authctx.NewVerifier(authctx.Config{Audiences: []string{"capability-core"}, Issuer: "test-issuer", PublicKeyPEM: publicKey})
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"org_id": input.organizationID, "service_id": input.serviceID, "principal_type": "service",
		"scopes": []string{input.scope}, "zdr": input.zdr, "iss": "test-issuer", "sub": input.serviceID,
		"aud": []string{"capability-core"}, "iat": now.Unix(), "nbf": now.Unix(), "exp": now.Add(time.Minute).Unix(),
	}).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	handler := &CapabilitiesHandler{availabilityStore: store}
	mux := http.NewServeMux()
	handler.Register(mux)
	request := httptest.NewRequest(http.MethodPost, input.path, strings.NewReader(input.body))
	request.Header.Set("Authorization", "Bearer "+token)
	recorder := httptest.NewRecorder()
	verifier.HTTPMiddleware(authz.AuthorizeHTTP)(mux).ServeHTTP(recorder, request)
	return recorder
}

func TestBrowserBrokerAttestsBrowserOpenOnItsOwnLane(t *testing.T) {
	store := &fakeAvailabilityStore{row: browserOpenRow("global"), updated: true}
	recorder := signedAttestationRecorder(t, store, browserBrokerAttestation())
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.auditAction != "global_availability_attested" {
		t.Fatalf("audit action = %q, want the global lane", store.auditAction)
	}
	if store.gotUpdate.ExpectedVersion != "1" || store.gotUpdate.ExecutionMode != models.ExecutionAgentic ||
		store.gotUpdate.HealthCheckedAt.IsZero() {
		t.Fatalf("update = %+v", store.gotUpdate)
	}
	// A high-risk capability is never stored or served as plainly available:
	// the attestation becomes approval_required.
	if store.gotUpdate.State != string(models.AvailabilityApprovalRequired) {
		t.Fatalf("stored state = %q, want approval_required", store.gotUpdate.State)
	}
	var response struct {
		Data struct {
			ID               string `json:"id"`
			State            string `json:"state"`
			RequiresApproval bool   `json:"requires_approval"`
			ExecutionMode    string `json:"execution_mode"`
		} `json:"data"`
	}
	if err := json.NewDecoder(recorder.Body).Decode(&response); err != nil {
		t.Fatal(err)
	}
	if response.Data.ID != browserOwnerCapabilityID || response.Data.State != string(models.AvailabilityApprovalRequired) ||
		!response.Data.RequiresApproval || response.Data.ExecutionMode != models.ExecutionAgentic {
		t.Fatalf("response = %+v", response.Data)
	}
}

func TestBrowserHealthLaneRefusesEveryOtherCaller(t *testing.T) {
	tests := []struct {
		name  string
		row   *registry.CapabilityRow
		input func(signedAttestation) signedAttestation
	}{
		{
			name: "execution-core with the global health scope on the browser route",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.serviceID, in.scope = authz.ExecutionCoreServiceID, authz.GlobalHealthWriteScope
				return in
			},
		},
		{
			name: "conversation-core with the owner-action scope on the browser route",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.serviceID, in.scope = authz.ConversationCoreServiceID, authz.OwnerActionHealthWriteScope
				return in
			},
		},
		{
			name: "an unrelated service holding the browser health scope",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.serviceID = "service:unrelated"
				return in
			},
		},
		{
			name: "the unprefixed deployment name",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.serviceID = "browser-broker"
				return in
			},
		},
		{
			name: "browser-broker without its scope",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.scope = authz.HealthWriteScope
				return in
			},
		},
		{
			name: "browser-broker attesting another capability",
			row: &registry.CapabilityRow{
				ID: "cap.tool.http", OrgID: "global", Version: "1", Enabled: true, RiskLevel: models.RiskLow,
			},
			input: func(in signedAttestation) signedAttestation {
				in.body = browserHealthBody("cap.tool.http", models.ExecutionAgentic)
				return in
			},
		},
		{
			name: "browser-broker bound to a tenant organization",
			row:  browserOpenRow("acme"),
			input: func(in signedAttestation) signedAttestation {
				in.organizationID = "acme"
				return in
			},
		},
		{
			name: "browser-broker with a zero-retention token",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.zdr = true
				return in
			},
		},
		{
			name: "the generic route attesting cap.browser.open as execution-core",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.path = "/api/v1/capabilities/availability"
				in.serviceID, in.scope = authz.ExecutionCoreServiceID, authz.GlobalHealthWriteScope
				return in
			},
		},
		{
			name: "the generic route attesting cap.browser.open as browser-broker",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.path = "/api/v1/capabilities/availability"
				return in
			},
		},
		{
			name: "the tenant health route against an org row named cap.browser.open",
			row:  browserOpenRow("acme"),
			input: func(in signedAttestation) signedAttestation {
				in.path = "/api/v1/capabilities/availability"
				in.organizationID, in.serviceID, in.scope = "acme", "service:tenant-health", authz.HealthWriteScope
				return in
			},
		},
		{
			name: "the owner-action route attesting cap.browser.open as conversation-core",
			row:  browserOpenRow("global"),
			input: func(in signedAttestation) signedAttestation {
				in.path = "/api/v1/capabilities/owner-actions/health"
				in.serviceID, in.scope = authz.ConversationCoreServiceID, authz.OwnerActionHealthWriteScope
				return in
			},
		},
	}
	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			store := &fakeAvailabilityStore{row: test.row, updated: true}
			recorder := signedAttestationRecorder(t, store, test.input(browserBrokerAttestation()))
			// The owner-action route answers 503 before it authorizes when no
			// Control verifier is wired; every other refusal here is a 403.
			if recorder.Code != http.StatusForbidden && recorder.Code != http.StatusServiceUnavailable {
				t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
			}
			if store.auditAction != "" {
				t.Fatalf("refused attestation reached persistence: %q", store.auditAction)
			}
		})
	}
}

func TestOwnerActionLaneWithItsVerifierStillRefusesBrowserOpen(t *testing.T) {
	store := &fakeAvailabilityStore{row: browserOpenRow("global"), updated: true}
	handler := (&CapabilitiesHandler{availabilityStore: store}).WithModelActionViewVerifier(&ControlModelActionViewVerifier{
		keyID: "control-key", public: make([]byte, 32),
	})
	request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/owner-actions/health",
		strings.NewReader(browserHealthBody(browserOwnerCapabilityID, models.ExecutionAgentic)))
	request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), authctx.Principal{
		OrganizationID: "global", ActorID: authz.ConversationCoreServiceID, PrincipalType: "service",
		Scopes: []string{authz.OwnerActionHealthWriteScope}, RetentionPolicyPresent: true,
	}))
	recorder := httptest.NewRecorder()
	handler.attestOwnerActionHealth(recorder, request)
	if recorder.Code != http.StatusForbidden || store.auditAction != "" {
		t.Fatalf("owner-action lane on cap.browser.open = %d/%q", recorder.Code, store.auditAction)
	}
}

// The handler re-checks the lane's predicates itself, so a miswired route that
// skipped the HTTP authorizer still cannot open the lane to another caller.
func TestBrowserHealthHandlerRechecksTheOwnerWithoutTheAuthorizer(t *testing.T) {
	for name, principal := range map[string]authctx.Principal{
		"execution-core": {OrganizationID: "global", ActorID: authz.ExecutionCoreServiceID, PrincipalType: "service", Scopes: []string{authz.GlobalHealthWriteScope, authz.BrowserHealthWriteScope}},
		"tenant org":     {OrganizationID: "acme", ActorID: authz.BrowserBrokerServiceID, PrincipalType: "service", Scopes: []string{authz.BrowserHealthWriteScope}},
		"user":           {OrganizationID: "global", ActorID: authz.BrowserBrokerServiceID, PrincipalType: "user", Scopes: []string{authz.BrowserHealthWriteScope}},
		"no scope":       {OrganizationID: "global", ActorID: authz.BrowserBrokerServiceID, PrincipalType: "service"},
	} {
		principal := principal
		t.Run(name, func(t *testing.T) {
			store := &fakeAvailabilityStore{row: browserOpenRow(principal.OrganizationID), updated: true}
			handler := &CapabilitiesHandler{availabilityStore: store}
			request := httptest.NewRequest(http.MethodPost, browserHealthPath,
				strings.NewReader(browserHealthBody(browserOwnerCapabilityID, models.ExecutionAgentic)))
			request = request.WithContext(authctx.ContextWithPrincipal(request.Context(), principal))
			recorder := httptest.NewRecorder()
			handler.attestBrowserOwnerHealth(recorder, request)
			if recorder.Code != http.StatusForbidden || store.auditAction != "" {
				t.Fatalf("status = %d, audit = %q", recorder.Code, store.auditAction)
			}
		})
	}
}

// An available attestation without a usable execution mode is refused with
// 422, and one without the agentic mode never makes the high-risk browser
// runnable: it is stored as approval_path_unavailable.
func TestBrowserHealthLaneNeedsTheAgenticExecutionMode(t *testing.T) {
	store := &fakeAvailabilityStore{row: browserOpenRow("global"), updated: true}
	input := browserBrokerAttestation()
	input.body = browserHealthBody(browserOwnerCapabilityID, models.ExecutionUnavailable)
	recorder := signedAttestationRecorder(t, store, input)
	if recorder.Code != http.StatusUnprocessableEntity || store.auditAction != "" {
		t.Fatalf("available with execution_mode unavailable = %d/%q: %s", recorder.Code, store.auditAction, recorder.Body.String())
	}

	store = &fakeAvailabilityStore{row: browserOpenRow("global"), updated: true}
	input.body = browserHealthBody(browserOwnerCapabilityID, models.ExecutionDirectRead)
	recorder = signedAttestationRecorder(t, store, input)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.gotUpdate.State == string(models.AvailabilityApprovalRequired) ||
		store.gotUpdate.State == string(models.AvailabilityAvailable) ||
		store.gotUpdate.ReasonCode != "approval_path_unavailable" {
		t.Fatalf("direct_read attestation made the browser runnable: %+v", store.gotUpdate)
	}
}

func TestBrowserHealthLaneRecordsAnUnhealthyBroker(t *testing.T) {
	store := &fakeAvailabilityStore{row: browserOpenRow("global"), updated: true}
	input := browserBrokerAttestation()
	input.body = `{"id":"cap.browser.open","version":"1","state":"unhealthy","reason_code":"quarry_edge_not_ready",` +
		`"reason":"","execution_mode":"unavailable","cost_class":"variable"}`
	recorder := signedAttestationRecorder(t, store, input)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.gotUpdate.State != string(models.AvailabilityUnhealthy) || store.gotUpdate.ReasonCode != "quarry_edge_not_ready" {
		t.Fatalf("update = %+v", store.gotUpdate)
	}
}

func TestBrowserHealthLaneKeepsTheVersionGuard(t *testing.T) {
	store := &fakeAvailabilityStore{row: browserOpenRow("global"), updated: true}
	store.row.Version = "2"
	recorder := signedAttestationRecorder(t, store, browserBrokerAttestation())
	if recorder.Code != http.StatusUnprocessableEntity || store.auditAction != "" {
		t.Fatalf("stale version = %d/%q", recorder.Code, store.auditAction)
	}
}
