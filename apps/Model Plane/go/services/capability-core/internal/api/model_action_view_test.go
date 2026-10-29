package api

import (
	"bytes"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/capability-core/internal/authz"
	"github.com/triodelab/model-plane/services/capability-core/internal/models"
	"github.com/triodelab/model-plane/services/capability-core/internal/registry"
)

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

func validControlModelActionView(now time.Time) controlModelActionView {
	return controlModelActionView{
		DecisionRef: "view-1", RunID: "run-1", ThreadID: "thread-1", OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1",
		ServiceAudience: modelActionViewServiceAudience, ActionID: ticketsCreateToolName, ActionSchemaHash: ticketsCreateActionSchemaHash,
		RecipientAudienceRef: "audience:space-1:1", RecipientAudienceHash: "sha256:audience", RecipientAudienceRevision: 1,
		PrivacyPolicyRef: "privacy:org-1:1", RunContextAuthorizationRef: "control:space-1:thread-create:1", AuthorityRevision: 1,
		Permissions: []string{modelActionViewPermission}, Purpose: "support", LawfulBasis: "contract", PrivacyClass: "internal",
		ThirdPartyAllowed: false, RetentionClass: "standard", Residency: "eu", DeletionScope: "space", ZeroDataRetention: false,
		IssuedAt: now.Add(-time.Minute), ExpiresAt: now.Add(time.Minute), Nonce: "nonce-1",
	}
}

func signControlModelActionView(t *testing.T, private ed25519.PrivateKey, keyID string, view controlModelActionView) string {
	t.Helper()
	payload, err := json.Marshal(view)
	if err != nil {
		t.Fatal(err)
	}
	signed := modelActionViewVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
}
