package api

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"os"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

// fakeAgentActionAdmissions answers ListEnabled from its rows. It does not filter by status, so
// the handler's own enabled check is what these tests exercise.
type fakeAgentActionAdmissions struct {
	rows []*registry.AgentActionAdmission
}

func (f *fakeAgentActionAdmissions) ListEnabled(_ context.Context, ids []string) ([]*registry.AgentActionAdmission, error) {
	var out []*registry.AgentActionAdmission
	for _, row := range f.rows {
		if slices.Contains(ids, row.ActionID) {
			out = append(out, row)
		}
	}
	return out, nil
}

func TestModelActionViewFailsClosedWithoutControlVerifier(t *testing.T) {
	handler := &CapabilitiesHandler{availabilityStore: &fakeAvailabilityStore{}}
	request := httptest.NewRequest(http.MethodPost, modelActionViewPath, bytes.NewBufferString(`{"run_id":"run-1","control_view_token":"signed"}`))
	recorder := httptest.NewRecorder()
	handler.resolveModelActionView(recorder, request)
	if recorder.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
}

func TestModelActionViewIsRunBoundAndUnavailableUntilOwnerHealthIsAttested(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	store := &fakeAvailabilityStore{row: &registry.CapabilityRow{
		ID: "cap.tool.ticket.create", OrgID: "global", Version: "1",
		Enabled: true, RiskLevel: models.RiskHigh,
	}}
	handler := (&CapabilitiesHandler{availabilityStore: store}).WithModelActionViewVerifier(verifier)

	view := validControlModelActionView(now)
	token := signControlModelActionView(t, private, "control-key", view)
	recorder := modelActionViewRecorder(t, handler, "run-1", token)
	if recorder.Code != http.StatusOK {
		t.Fatalf("unattested status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	var unavailable modelActionViewResponse
	if err := json.NewDecoder(recorder.Body).Decode(&unavailable); err != nil {
		t.Fatal(err)
	}
	if len(unavailable.Data.Actions) != 0 || unavailable.Data.ReasonCode != "health_not_attested" {
		t.Fatalf("unattested model view = %#v", unavailable)
	}

	checkedAt := now
	store.row.AvailabilityState = string(models.AvailabilityAvailable)
	store.row.ExecutionMode = models.ExecutionAgentic
	store.row.CostClass = models.CostBounded
	store.row.HealthCheckedAt = &checkedAt
	recorder = modelActionViewRecorder(t, handler, "run-1", token)
	if recorder.Code != http.StatusOK {
		t.Fatalf("attested status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	var available modelActionViewResponse
	if err := json.NewDecoder(recorder.Body).Decode(&available); err != nil {
		t.Fatal(err)
	}
	if len(available.Data.Actions) != 1 || available.Data.Actions[0].Name != ticketsCreateToolName ||
		available.Data.Actions[0].ActionSchemaHash != ticketsCreateActionSchemaHash ||
		available.Data.Actions[0].RequiresApproval != true {
		t.Fatalf("attested model view = %#v", available)
	}

	recorder = modelActionViewRecorder(t, handler, "another-run", token)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("cross-run replay status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
}

// The v2 view under the admission table offers tickets.create with the same definition bytes a
// v1 view under tickets_only offers, so turning the table on cannot change what the Model sees
// for the one action that ships today.
func TestModelActionViewTicketsCreateIsByteIdenticalUnderV2(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	checkedAt := now

	legacyVerifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	legacyHandler := (&CapabilitiesHandler{availabilityStore: attestedTicketsStore(checkedAt)}).WithModelActionViewVerifier(legacyVerifier)
	legacy := modelActionViewRecorder(t, legacyHandler, "run-1",
		signControlModelActionView(t, private, "control-key", validControlModelActionView(now)))
	if legacy.Code != http.StatusOK {
		t.Fatalf("v1 status = %d, body = %s", legacy.Code, legacy.Body.String())
	}

	tableVerifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	tableVerifier.WithAgentActionAdmissions(&fakeAgentActionAdmissions{rows: []*registry.AgentActionAdmission{ticketsCreateAdmission()}}, true)
	tableHandler := (&CapabilitiesHandler{availabilityStore: attestedTicketsStore(checkedAt)}).WithModelActionViewVerifier(tableVerifier)
	table := modelActionViewRecorder(t, tableHandler, "run-1",
		signControlModelActionViewV2(t, private, "control-key", validControlModelActionViewV2(now, map[string]string{
			ticketsCreateToolName: ticketsCreateActionSchemaHash,
		})))
	if table.Code != http.StatusOK {
		t.Fatalf("v2 status = %d, body = %s", table.Code, table.Body.String())
	}

	var legacyView, tableView modelActionViewResponse
	if err := json.Unmarshal(legacy.Body.Bytes(), &legacyView); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(table.Body.Bytes(), &tableView); err != nil {
		t.Fatal(err)
	}
	legacyActions, err := json.Marshal(legacyView.Data.Actions)
	if err != nil {
		t.Fatal(err)
	}
	tableActions, err := json.Marshal(tableView.Data.Actions)
	if err != nil {
		t.Fatal(err)
	}
	if len(legacyView.Data.Actions) != 1 || string(legacyActions) != string(tableActions) {
		t.Fatalf("tickets.create definition differs:\nv1 tickets_only = %s\nv2 table       = %s", legacyActions, tableActions)
	}
}

func TestModelActionViewOffersOnlyEnabledRowsInTheSignedList(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	verifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	declared := *ticketsCreateAdmission()
	declared.ActionID = "brreg.lookup_organization"
	declared.ToolName = "brreg.lookup_organization"
	declared.CapabilityID = "cap.tool.information.read"
	declared.ActionSchemaHash = "sha256:" + strings.Repeat("a", 64)
	declared.Status = "declared"
	unsigned := *ticketsCreateAdmission()
	unsigned.ActionID = "shipping.get_quotes"
	unsigned.ToolName = "shipping.get_quotes"
	unsigned.Status = "enabled"
	verifier.WithAgentActionAdmissions(&fakeAgentActionAdmissions{rows: []*registry.AgentActionAdmission{ticketsCreateAdmission(), &declared, &unsigned}}, true)
	handler := (&CapabilitiesHandler{availabilityStore: attestedTicketsStore(now)}).WithModelActionViewVerifier(verifier)

	token := signControlModelActionViewV2(t, private, "control-key", validControlModelActionViewV2(now, map[string]string{
		ticketsCreateToolName:       ticketsCreateActionSchemaHash,
		"brreg.lookup_organization": declared.ActionSchemaHash,
	}))
	recorder := modelActionViewRecorder(t, handler, "run-1", token)
	if recorder.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
	var offered modelActionViewResponse
	if err := json.NewDecoder(recorder.Body).Decode(&offered); err != nil {
		t.Fatal(err)
	}
	if len(offered.Data.Actions) != 1 || offered.Data.Actions[0].Name != ticketsCreateToolName {
		t.Fatalf("offered = %#v, want only the enabled, signed tickets.create", offered.Data.Actions)
	}
}

func TestModelActionViewRejectsHashMismatch(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	verifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	verifier.WithAgentActionAdmissions(&fakeAgentActionAdmissions{rows: []*registry.AgentActionAdmission{ticketsCreateAdmission()}}, true)
	handler := (&CapabilitiesHandler{availabilityStore: attestedTicketsStore(now)}).WithModelActionViewVerifier(verifier)

	wrong := "sha256:" + strings.Repeat("0", 64)
	token := signControlModelActionViewV2(t, private, "control-key", validControlModelActionViewV2(now, map[string]string{
		ticketsCreateToolName: wrong,
	}))
	recorder := modelActionViewRecorder(t, handler, "run-1", token)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("hash mismatch status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
}

func TestModelActionViewV2IsRefusedWhileTheTableIsOff(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	verifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	handler := (&CapabilitiesHandler{availabilityStore: attestedTicketsStore(now)}).WithModelActionViewVerifier(verifier)

	token := signControlModelActionViewV2(t, private, "control-key", validControlModelActionViewV2(now, map[string]string{
		ticketsCreateToolName: ticketsCreateActionSchemaHash,
	}))
	recorder := modelActionViewRecorder(t, handler, "run-1", token)
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("v2 with table off status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
}

func TestModelActionViewStillRefusesZeroDataRetention(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	verifier, err := NewControlModelActionViewVerifier("control-key", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	handler := (&CapabilitiesHandler{availabilityStore: attestedTicketsStore(now)}).WithModelActionViewVerifier(verifier)

	view := validControlModelActionView(now)
	view.ZeroDataRetention = true
	recorder := modelActionViewRecorder(t, handler, "run-1", signControlModelActionView(t, private, "control-key", view))
	if recorder.Code != http.StatusForbidden {
		t.Fatalf("zdr view status = %d, body = %s", recorder.Code, recorder.Body.String())
	}
}

// The migration seeds the tickets.create row. The constants and the seeded row must describe the
// same action, or the table would offer a different definition than the one Execution Core pins.
func TestAgentActionAdmissionSeedCarriesTheTicketsCreateContract(t *testing.T) {
	seed, err := os.ReadFile("../../migrations/0022_agent_action_admissions.up.sql")
	if err != nil {
		t.Fatal(err)
	}
	for name, value := range map[string]string{
		"parameters":  ticketsCreateParametersJSON,
		"hash":        ticketsCreateActionSchemaHash,
		"description": ticketsCreateDescription,
	} {
		if !strings.Contains(string(seed), value) {
			t.Fatalf("migration 0022 does not carry the tickets.create %s", name)
		}
	}
}

func attestedTicketsStore(checkedAt time.Time) *fakeAvailabilityStore {
	return &fakeAvailabilityStore{row: &registry.CapabilityRow{
		ID: ticketsCreateCapabilityID, OrgID: "global", Version: "1",
		Enabled: true, RiskLevel: models.RiskHigh,
		AvailabilityState: string(models.AvailabilityAvailable),
		ExecutionMode:     models.ExecutionAgentic,
		CostClass:         models.CostBounded,
		HealthCheckedAt:   &checkedAt,
	}}
}

func modelActionViewRecorder(t *testing.T, handler *CapabilitiesHandler, runID, token string) *httptest.ResponseRecorder {
	t.Helper()
	request := httptest.NewRequest(http.MethodPost, modelActionViewPath, bytes.NewBufferString(`{"run_id":"`+runID+`","control_view_token":"`+token+`"}`))
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicKey := pem.EncodeToMemory(&pem.Block{Type: "RSA PUBLIC KEY", Bytes: x509.MarshalPKCS1PublicKey(&key.PublicKey)})
	verifier, err := authctx.NewVerifier(authctx.Config{Audiences: []string{"capability-core"}, Issuer: "test-issuer", PublicKeyPEM: publicKey})
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	bearer, err := jwt.NewWithClaims(jwt.SigningMethodRS256, jwt.MapClaims{
		"org_id": "org-1", "service_id": authz.ExecutionCoreServiceID, "principal_type": "service", "scopes": []string{"capability:model-action:view"},
		"zdr": false, "iss": "test-issuer", "sub": authz.ExecutionCoreServiceID, "aud": []string{"capability-core"},
		"iat": now.Unix(), "nbf": now.Unix(), "exp": now.Add(time.Minute).Unix(),
	}).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	request.Header.Set("Authorization", "Bearer "+bearer)
	recorder := httptest.NewRecorder()
	verifier.HTTPMiddleware(authz.AuthorizeHTTP)(http.HandlerFunc(handler.resolveModelActionView)).ServeHTTP(recorder, request)
	return recorder
}

func validControlModelActionAuthority(now time.Time) controlModelActionAuthority {
	return controlModelActionAuthority{
		DecisionRef: "view-1", RunID: "run-1", ThreadID: "thread-1", OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1",
		ServiceAudience:      modelActionViewServiceAudience,
		RecipientAudienceRef: "audience:space-1:1", RecipientAudienceHash: "sha256:audience", RecipientAudienceRevision: 1,
		PrivacyPolicyRef: "privacy:org-1:1", RunContextAuthorizationRef: "control:space-1:thread-create:1", AuthorityRevision: 1,
		Permissions: []string{modelActionViewPermission}, Purpose: "support", LawfulBasis: "contract", PrivacyClass: "internal",
		ThirdPartyAllowed: false, RetentionClass: "standard", Residency: "eu", DeletionScope: "space", ZeroDataRetention: false,
		IssuedAt: now.Add(-time.Minute), ExpiresAt: now.Add(time.Minute), Nonce: "nonce-1",
	}
}

func validControlModelActionView(now time.Time) controlModelActionView {
	return controlModelActionView{
		controlModelActionAuthority: validControlModelActionAuthority(now),
		ActionID:                    ticketsCreateToolName,
		ActionSchemaHash:            ticketsCreateActionSchemaHash,
	}
}

func validControlModelActionViewV2(now time.Time, hashes map[string]string) controlModelActionViewV2 {
	return controlModelActionViewV2{
		controlModelActionAuthority: validControlModelActionAuthority(now),
		ActionSchemaHashes:          hashes,
	}
}

func signControlModelActionView(t *testing.T, private ed25519.PrivateKey, keyID string, view controlModelActionView) string {
	t.Helper()
	return signModelActionViewEnvelope(t, private, keyID, modelActionViewVersion, view)
}

func signControlModelActionViewV2(t *testing.T, private ed25519.PrivateKey, keyID string, view controlModelActionViewV2) string {
	t.Helper()
	return signModelActionViewEnvelope(t, private, keyID, modelActionViewVersionV2, view)
}

func signModelActionViewEnvelope(t *testing.T, private ed25519.PrivateKey, keyID, version string, view any) string {
	t.Helper()
	payload, err := json.Marshal(view)
	if err != nil {
		t.Fatal(err)
	}
	signed := version + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
}
