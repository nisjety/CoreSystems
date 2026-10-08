package api

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"errors"
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

type fakeAvailabilityStore struct {
	row         *registry.CapabilityRow
	getErr      error
	updateErr   error
	updated     bool
	gotOrgID    string
	gotUpdate   registry.AvailabilityUpdate
	auditAction string
}

func TestGlobalHealthAttestationRequiresDedicatedServiceScope(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name      string
		principal authctx.Principal
		want      bool
	}{
		{name: "exact global execution health authority", principal: authctx.Principal{OrganizationID: "global", ActorID: authz.ExecutionCoreServiceID, PrincipalType: "service", Scopes: []string{authz.GlobalHealthWriteScope}}, want: true},
		{name: "global scope on tenant execution workload is not global authority", principal: authctx.Principal{OrganizationID: "org-a", ActorID: authz.ExecutionCoreServiceID, PrincipalType: "service", Scopes: []string{authz.GlobalHealthWriteScope}}},
		{name: "global scope on unrelated global workload is not global authority", principal: authctx.Principal{OrganizationID: "global", ActorID: "service:unrelated", PrincipalType: "service", Scopes: []string{authz.GlobalHealthWriteScope}}},
		{name: "tenant health reporter is not global authority", principal: authctx.Principal{OrganizationID: "org-a", ActorID: "health", PrincipalType: "service", Scopes: []string{authz.HealthWriteScope}}},
		{name: "global catalog writer is not health authority", principal: authctx.Principal{OrganizationID: "ops", ActorID: "admin", PrincipalType: "service", Scopes: []string{authz.GlobalWriteScope}}},
		{name: "user is never global authority", principal: authctx.Principal{OrganizationID: "ops", ActorID: "user-a", PrincipalType: "user", Scopes: []string{authz.GlobalHealthWriteScope}}},
	}

	for _, test := range tests {
		test := test
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			if got := mayAttestGlobalCapability(test.principal); got != test.want {
				t.Fatalf("mayAttestGlobalCapability() = %v, want %v", got, test.want)
			}
		})
	}
}

func TestGenericGlobalHealthAttestorCannotAttestOwnerActionTicket(t *testing.T) {
	t.Parallel()

	principal := authctx.Principal{
		OrganizationID: "global",
		ActorID:        genericGlobalHealthAttesterID,
		PrincipalType:  "service",
		Scopes:         []string{authz.GlobalHealthWriteScope},
	}
	if mayUseGenericGlobalHealthAttestation(principal, ownerActionTicketCapabilityID) {
		t.Fatal("generic global health attestor may attest owner-action ticket capability")
	}
	if !mayUseGenericGlobalHealthAttestation(principal, "cap.command.sandbox") {
		t.Fatal("generic global health attestor may not attest its ordinary runtime capability")
	}
	unprefixed := principal
	unprefixed.ActorID = "execution-core"
	if mayUseGenericGlobalHealthAttestation(unprefixed, "cap.command.sandbox") {
		t.Fatal("unprefixed deployment name was accepted as the signed health identity")
	}
	// cap.retrieval.query used to be the "unmeasured" negative control here. It
	// is now measured (execution-core probes the retrieval engine), so the
	// control moves to an id execution-core still has no probe for.
	if mayUseGenericGlobalHealthAttestation(principal, "cap.tool.provider.execute") {
		t.Fatal("generic global health attestor may attest an unmeasured capability")
	}
	otherService := principal
	otherService.ActorID = "capability-health-attestor"
	if mayUseGenericGlobalHealthAttestation(otherService, "cap.command.sandbox") {
		t.Fatal("an unrelated global-health service may attest execution-core capability health")
	}
}

// The two ids added for knowledge_search and web_search/web_fetch. Each one is
// executed inside execution-core's own tool loop (migration 0008 records
// config_json.owner_service = execution-core) and execution-core probes the
// real dependency before it attests — the bar cap.memory.* and cap.agent.spawn
// already passed.
var executionOwnedDependencyBackedCapabilityIDs = []string{"cap.retrieval.query", "cap.tool.http"}

func executionCoreGlobalHealthPrincipal() authctx.Principal {
	return authctx.Principal{
		OrganizationID: "global",
		ActorID:        authz.ExecutionCoreServiceID,
		PrincipalType:  "service",
		Scopes:         []string{authz.GlobalHealthWriteScope},
	}
}

func TestGenericGlobalHealthAttestorMayAttestRetrievalAndWebOnlyAsExecutionCore(t *testing.T) {
	t.Parallel()

	exact := executionCoreGlobalHealthPrincipal()
	for _, capabilityID := range executionOwnedDependencyBackedCapabilityIDs {
		capabilityID := capabilityID
		t.Run(capabilityID, func(t *testing.T) {
			t.Parallel()
			if !mayUseGenericGlobalHealthAttestation(exact, capabilityID) {
				t.Fatalf("service:execution-core in org global with the global-health scope may not attest %s", capabilityID)
			}

			wrongActors := map[string]authctx.Principal{
				"unprefixed deployment name": func() authctx.Principal {
					p := exact
					p.ActorID = "execution-core"
					return p
				}(),
				"unrelated global service": func() authctx.Principal {
					p := exact
					p.ActorID = "service:unrelated"
					return p
				}(),
				"conversation-core owner-action workload": func() authctx.Principal {
					p := exact
					p.ActorID = authz.ConversationCoreServiceID
					return p
				}(),
				"execution-core in a tenant org": func() authctx.Principal {
					p := exact
					p.OrganizationID = "org-a"
					return p
				}(),
				"execution-core without the global-health scope": func() authctx.Principal {
					p := exact
					p.Scopes = []string{authz.HealthWriteScope}
					return p
				}(),
				"a user holding the global-health scope": func() authctx.Principal {
					p := exact
					p.PrincipalType = "user"
					return p
				}(),
			}
			for name, principal := range wrongActors {
				if mayUseGenericGlobalHealthAttestation(principal, capabilityID) {
					t.Fatalf("%s was allowed to attest %s", name, capabilityID)
				}
			}
		})
	}
}

// Widening this list is a decision, so the exact set is pinned: a future id
// has to be added here on purpose and justified, not slip in beside another
// edit. Effectful or externally owned capabilities must never appear.
func TestGenericGlobalHealthAllowlistIsExactlyTheExecutionOwnedRuntimes(t *testing.T) {
	t.Parallel()

	want := map[string]struct{}{
		"cap.command.sandbox":    {},
		"cap.command.shell":      {},
		"cap.memory.search":      {},
		"cap.memory.index":       {},
		"cap.agent.spawn":        {},
		"cap.process.background": {},
		"cap.retrieval.query":    {},
		"cap.tool.http":          {},
	}
	for capabilityID := range want {
		if _, present := genericGlobalHealthCapabilityIDs[capabilityID]; !present {
			t.Errorf("allowlist is missing %s", capabilityID)
		}
	}
	for capabilityID := range genericGlobalHealthCapabilityIDs {
		if _, expected := want[capabilityID]; !expected {
			t.Errorf("allowlist grew by %s; justify it that execution-core owns and probes that runtime, then pin it here", capabilityID)
		}
	}
}

func TestGenericGlobalHealthAttestorStillCannotAttestEveryOtherCapability(t *testing.T) {
	t.Parallel()

	principal := executionCoreGlobalHealthPrincipal()
	for _, capabilityID := range []string{
		ownerActionTicketCapabilityID,
		"cap.tool.provider.execute",
		"cap.tool.provider.read",
		"cap.tool.social.publish",
		"cap.tool.social.read",
		"cap.tool.shipping.book",
		"cap.tool.shipping.read",
		"cap.tool.shipping.track",
		"cap.tool.information.read",
		"cap.browser.open",
		"cap.skill.summarize",
		"cap.agent.lineage.read",
		"cap.retrieval.query.extra",
		"",
	} {
		if mayUseGenericGlobalHealthAttestation(principal, capabilityID) {
			t.Errorf("generic global health attestor may attest %q", capabilityID)
		}
	}
}

func TestExecutionCoreAttestationForRetrievalAndWebIsAcceptedAndAudited(t *testing.T) {
	for _, test := range []struct {
		capabilityID string
		risk         string
	}{
		{capabilityID: "cap.retrieval.query", risk: models.RiskLow},
		{capabilityID: "cap.tool.http", risk: models.RiskMedium},
	} {
		test := test
		t.Run(test.capabilityID, func(t *testing.T) {
			store := &fakeAvailabilityStore{
				row: &registry.CapabilityRow{
					ID: test.capabilityID, OrgID: "global", Version: "1",
					Enabled: true, RiskLevel: test.risk,
				},
				updated: true,
			}
			recorder := availabilityAttestationRecorder(t, store, test.capabilityID, "global", authz.ExecutionCoreServiceID, authz.GlobalHealthWriteScope)
			if recorder.Code != http.StatusOK {
				t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
			}
			if store.auditAction != "global_availability_attested" {
				t.Fatalf("audit action = %q", store.auditAction)
			}
			// Attesting health is not granting permission: neither row is
			// high-risk, so the stored state stays plain `available`.
			if store.gotUpdate.State != string(models.AvailabilityAvailable) {
				t.Fatalf("stored state = %q", store.gotUpdate.State)
			}
		})
	}
}

func TestOtherActorsStillCannotPersistRetrievalOrWebAttestation(t *testing.T) {
	for _, capabilityID := range executionOwnedDependencyBackedCapabilityIDs {
		for _, actor := range []struct {
			name, org, service, scope string
			want                      int
		}{
			// The tenant lane cannot even see a global row: not found, and
			// nothing is written.
			{name: "tenant health reporter", org: "org-a", service: "tenant-health", scope: authz.HealthWriteScope, want: http.StatusNotFound},
			{name: "tenant health reporter on the global org", org: "global", service: "tenant-health", scope: authz.HealthWriteScope, want: http.StatusForbidden},
			{name: "unrelated global service", org: "global", service: "service:unrelated", scope: authz.GlobalHealthWriteScope, want: http.StatusForbidden},
			{name: "execution-core in a tenant org", org: "org-a", service: authz.ExecutionCoreServiceID, scope: authz.GlobalHealthWriteScope, want: http.StatusForbidden},
		} {
			capabilityID, actor := capabilityID, actor
			t.Run(capabilityID+"/"+actor.name, func(t *testing.T) {
				store := &fakeAvailabilityStore{
					row: &registry.CapabilityRow{
						ID: capabilityID, OrgID: "global", Version: "1",
						Enabled: true, RiskLevel: models.RiskMedium,
					},
					updated: true,
				}
				recorder := availabilityAttestationRecorder(t, store, capabilityID, actor.org, actor.service, actor.scope)
				if recorder.Code != actor.want {
					t.Fatalf("status = %d, want %d, body = %s", recorder.Code, actor.want, recorder.Body.String())
				}
				if store.auditAction != "" {
					t.Fatalf("%s reached persistence: %q", actor.name, store.auditAction)
				}
			})
		}
	}
}

func TestExecutionCoreStillCannotPersistAttestationForUnmeasuredCapabilities(t *testing.T) {
	for _, capabilityID := range []string{"cap.tool.provider.execute", "cap.browser.open", "cap.tool.social.publish", "cap.tool.shipping.book"} {
		capabilityID := capabilityID
		t.Run(capabilityID, func(t *testing.T) {
			store := &fakeAvailabilityStore{
				row: &registry.CapabilityRow{
					ID: capabilityID, OrgID: "global", Version: "1",
					Enabled: true, RiskLevel: models.RiskHigh,
				},
				updated: true,
			}
			recorder := availabilityAttestationRecorder(t, store, capabilityID, "global", authz.ExecutionCoreServiceID, authz.GlobalHealthWriteScope)
			if recorder.Code != http.StatusForbidden {
				t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
			}
			if store.auditAction != "" {
				t.Fatalf("unmeasured capability reached persistence: %q", store.auditAction)
			}
		})
	}
}

func TestGenericGlobalHealthEndpointCannotPersistOwnerActionTicket(t *testing.T) {
	store := &fakeAvailabilityStore{
		row: &registry.CapabilityRow{
			ID: "cap.tool.ticket.create", OrgID: "global", Version: "1",
			Enabled: true, RiskLevel: models.RiskHigh,
		},
		updated: true,
	}
	recorder := ownerActionAttestationRecorder(t, store, authz.GlobalHealthWriteScope)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.auditAction != "" {
		t.Fatalf("owner-action attestation reached persistence: %q", store.auditAction)
	}
}

func TestUnrelatedGlobalHealthServiceCannotPersistAllowlistedCapability(t *testing.T) {
	store := &fakeAvailabilityStore{
		row: &registry.CapabilityRow{
			ID: "cap.command.sandbox", OrgID: "global", Version: "1",
			Enabled: true, RiskLevel: models.RiskLow,
		},
		updated: true,
	}
	recorder := availabilityAttestationRecorder(t, store, "cap.command.sandbox", "global", "service:unrelated", authz.GlobalHealthWriteScope)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.auditAction != "" {
		t.Fatalf("unrelated global health service reached persistence: %q", store.auditAction)
	}
}

func TestTenantHealthEndpointCannotPersistGlobalOwnerActionTicket(t *testing.T) {
	store := &fakeAvailabilityStore{
		row: &registry.CapabilityRow{
			ID: "cap.tool.ticket.create", OrgID: "global", Version: "1",
			Enabled: true, RiskLevel: models.RiskHigh,
		},
		updated: true,
	}
	recorder := ownerActionAttestationRecorder(t, store, authz.HealthWriteScope)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.auditAction != "" {
		t.Fatalf("tenant health attestation reached persistence: %q", store.auditAction)
	}
}

func TestTenantHealthEndpointCannotPersistGlobalRuntimeCapability(t *testing.T) {
	store := &fakeAvailabilityStore{
		row: &registry.CapabilityRow{
			ID: "cap.command.sandbox", OrgID: "global", Version: "1",
			Enabled: true, RiskLevel: models.RiskLow,
		},
		updated: true,
	}
	recorder := availabilityAttestationRecorder(t, store, "cap.command.sandbox", "global", "tenant-health", authz.HealthWriteScope)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	if store.auditAction != "" {
		t.Fatalf("tenant health attestation reached global persistence: %q", store.auditAction)
	}
}

func TestDedicatedOwnerActionHealthAttestationNeedsExactOwnerAndControlVerifier(t *testing.T) {
	store := &fakeAvailabilityStore{
		row: &registry.CapabilityRow{
			ID: "cap.tool.ticket.create", OrgID: "global", Version: "1",
			Enabled: true, RiskLevel: models.RiskHigh,
		},
		updated: true,
	}
	withoutVerifier := &CapabilitiesHandler{availabilityStore: store}
	recorder := ownerActionHealthRecorder(t, withoutVerifier, authz.ConversationCoreServiceID, authz.OwnerActionHealthWriteScope)
	if recorder.Code != http.StatusServiceUnavailable || store.auditAction != "" {
		t.Fatalf("unwired owner health = %d/%q", recorder.Code, store.auditAction)
	}

	withVerifier := (&CapabilitiesHandler{availabilityStore: store}).WithModelActionViewVerifier(&ControlModelActionViewVerifier{
		keyID: "control-key", public: make([]byte, 32),
	})
	recorder = ownerActionHealthRecorder(t, withVerifier, authz.ConversationCoreServiceID, authz.OwnerActionHealthWriteScope)
	if recorder.Code != http.StatusOK || store.auditAction != "global_availability_attested" {
		t.Fatalf("dedicated owner health = %d/%q: %s", recorder.Code, store.auditAction, recorder.Body.String())
	}

	store.auditAction = ""
	recorder = ownerActionHealthRecorder(t, withVerifier, authz.ExecutionCoreServiceID, authz.OwnerActionHealthWriteScope)
	if recorder.Code != http.StatusForbidden || store.auditAction != "" {
		t.Fatalf("wrong owner health principal = %d/%q", recorder.Code, store.auditAction)
	}
}

func ownerActionAttestationRecorder(t *testing.T, store *fakeAvailabilityStore, scope string) *httptest.ResponseRecorder {
	return availabilityAttestationRecorder(t, store, ownerActionTicketCapabilityID, "global", authz.ConversationCoreServiceID, scope)
}

func availabilityAttestationRecorder(t *testing.T, store *fakeAvailabilityStore, capabilityID, organizationID, serviceID, scope string) *httptest.ResponseRecorder {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicKey := pem.EncodeToMemory(&pem.Block{
		Type: "RSA PUBLIC KEY", Bytes: x509.MarshalPKCS1PublicKey(&key.PublicKey),
	})
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences:    []string{"capability-core"},
		Issuer:       "test-issuer",
		PublicKeyPEM: publicKey,
	})
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"org_id":         organizationID,
		"service_id":     serviceID,
		"principal_type": "service",
		"scopes":         []string{scope},
		"zdr":            false,
		"iss":            "test-issuer",
		"sub":            serviceID,
		"aud":            []string{"capability-core"},
		"iat":            now.Unix(),
		"nbf":            now.Unix(),
		"exp":            now.Add(time.Minute).Unix(),
	}).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	handler := &CapabilitiesHandler{availabilityStore: store}
	secured := verifier.HTTPMiddleware(authz.AuthorizeHTTP)(http.HandlerFunc(handler.attestAvailability))
	requestBody := `{"id":"` + capabilityID + `","version":"1","state":"available","reason_code":"runtime_healthy","execution_mode":"agentic","cost_class":"bounded"}`
	request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/availability", strings.NewReader(requestBody))
	request.Header.Set("Authorization", "Bearer "+token)
	recorder := httptest.NewRecorder()
	secured.ServeHTTP(recorder, request)
	return recorder
}

func ownerActionHealthRecorder(t *testing.T, handler *CapabilitiesHandler, serviceID, scope string) *httptest.ResponseRecorder {
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
		"org_id": "global", "service_id": serviceID, "principal_type": "service", "scopes": []string{scope},
		"zdr": false, "iss": "test-issuer", "sub": serviceID, "aud": []string{"capability-core"},
		"iat": now.Unix(), "nbf": now.Unix(), "exp": now.Add(time.Minute).Unix(),
	}).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/owner-actions/health", strings.NewReader(`{
		"id":"cap.tool.ticket.create","version":"1","state":"available",
		"reason_code":"owner_contract_ready","execution_mode":"agentic","cost_class":"bounded"
	}`))
	request.Header.Set("Authorization", "Bearer "+token)
	recorder := httptest.NewRecorder()
	verifier.HTTPMiddleware(authz.AuthorizeHTTP)(http.HandlerFunc(handler.attestOwnerActionHealth)).ServeHTTP(recorder, request)
	return recorder
}

func (store *fakeAvailabilityStore) GetForOrg(_ context.Context, _ string, organizationID string) (*registry.CapabilityRow, error) {
	store.gotOrgID = organizationID
	if store.getErr != nil {
		return nil, store.getErr
	}
	return store.row, nil
}

func (store *fakeAvailabilityStore) GetGlobal(_ context.Context, _ string) (*registry.CapabilityRow, error) {
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

func (store *fakeAvailabilityStore) AttestAvailabilityGlobal(_ context.Context, _, _ string, update registry.AvailabilityUpdate) (bool, error) {
	store.gotUpdate = update
	store.auditAction = "global_availability_attested"
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
