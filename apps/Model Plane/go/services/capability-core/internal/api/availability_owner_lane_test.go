package api

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
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

// ownerActionHealthRecorderFor is ownerActionHealthRecorder for any owner-action capability. It signs a global token for
// serviceID with scope, sends the attestation for capabilityID to the owner-action route, and returns the response.
func ownerActionHealthRecorderFor(t *testing.T, handler *CapabilitiesHandler, capabilityID, serviceID, scope string) *httptest.ResponseRecorder {
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
	request := httptest.NewRequest(http.MethodPost, "/api/v1/capabilities/owner-actions/health", strings.NewReader(
		`{"id":"`+capabilityID+`","version":"1","state":"available","reason_code":"owner_contract_ready","execution_mode":"agentic","cost_class":"bounded"}`))
	request.Header.Set("Authorization", "Bearer "+token)
	recorder := httptest.NewRecorder()
	verifier.HTTPMiddleware(authz.AuthorizeHTTP)(http.HandlerFunc(handler.attestOwnerActionHealth)).ServeHTTP(recorder, request)
	return recorder
}

// WS18-S2: the owner-action lane attests exactly its attester table. Conversation Core attests the ticket; execution-core
// attests the three Model-owned Space rows and the schedule follow-up; wiki-store attests the two room canvas rows and nothing
// else; pause_self has no attester at all, so no caller attests it.
func TestOwnerActionLaneAttestsExactlyItsAttesterTable(t *testing.T) {
	const pauseSelfCapabilityID = "cap.tool.space.agent.pause_self"
	cases := []struct {
		name       string
		capability string
		service    string
		want       int
	}{
		{"conversation-core attests its ticket", ownerActionTicketCapabilityID, authz.ConversationCoreServiceID, http.StatusOK},
		{"execution-core may not attest the ticket", ownerActionTicketCapabilityID, authz.ExecutionCoreServiceID, http.StatusForbidden},
		{"execution-core attests thread rename", spaceThreadRenameCapabilityID, authz.ExecutionCoreServiceID, http.StatusOK},
		{"execution-core attests thread pin", spaceThreadPinCapabilityID, authz.ExecutionCoreServiceID, http.StatusOK},
		{"execution-core attests thread post_card", spaceThreadPostCardCapabilityID, authz.ExecutionCoreServiceID, http.StatusOK},
		{"execution-core attests schedule follow_up", spaceScheduleFollowUpCapabilityID, authz.ExecutionCoreServiceID, http.StatusOK},
		{"conversation-core may not attest thread rename", spaceThreadRenameCapabilityID, authz.ConversationCoreServiceID, http.StatusForbidden},
		{"conversation-core may not attest thread post_card", spaceThreadPostCardCapabilityID, authz.ConversationCoreServiceID, http.StatusForbidden},
		{"conversation-core may not attest schedule follow_up", spaceScheduleFollowUpCapabilityID, authz.ConversationCoreServiceID, http.StatusForbidden},
		{"conversation-core may not attest the room canvas read", spaceCanvasReadCapabilityID, authz.ConversationCoreServiceID, http.StatusForbidden},
		{"execution-core may not attest the room canvas write", spaceCanvasWriteCapabilityID, authz.ExecutionCoreServiceID, http.StatusForbidden},
		// P-CFG: wiki-store, the canvas owner, attests both canvas rows (its reporter is wiki-store-go/internal/capabilityhealth).
		{"wiki-store attests the room canvas read", spaceCanvasReadCapabilityID, authz.WikiStoreServiceID, http.StatusOK},
		{"wiki-store attests the room canvas write", spaceCanvasWriteCapabilityID, authz.WikiStoreServiceID, http.StatusOK},
		{"wiki-store may not attest thread rename", spaceThreadRenameCapabilityID, authz.WikiStoreServiceID, http.StatusForbidden},
		{"wiki-store may not attest the ticket", ownerActionTicketCapabilityID, authz.WikiStoreServiceID, http.StatusForbidden},
		{"conversation-core may not attest pause_self", pauseSelfCapabilityID, authz.ConversationCoreServiceID, http.StatusForbidden},
		{"execution-core may not attest pause_self", pauseSelfCapabilityID, authz.ExecutionCoreServiceID, http.StatusForbidden},
		{"an unrelated service may not attest the owner lane", spaceThreadRenameCapabilityID, "service:unrelated", http.StatusForbidden},
		// A-REC-03-TOOL: execution-core runs the directory resolver, so it is the row's only attester.
		{"execution-core attests the directory resolver", spaceDirectoryResolveCapabilityID, authz.ExecutionCoreServiceID, http.StatusOK},
		{"conversation-core may not attest the directory resolver", spaceDirectoryResolveCapabilityID, authz.ConversationCoreServiceID, http.StatusForbidden},
		{"wiki-store may not attest the directory resolver", spaceDirectoryResolveCapabilityID, authz.WikiStoreServiceID, http.StatusForbidden},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			store := &fakeAvailabilityStore{
				row:     &registry.CapabilityRow{ID: tc.capability, OrgID: "global", Version: "1", Enabled: true, RiskLevel: models.RiskLow},
				updated: true,
			}
			handler := (&CapabilitiesHandler{availabilityStore: store}).WithModelActionViewVerifier(&ControlModelActionViewVerifier{
				keyID: "control-key", public: make([]byte, 32),
			})
			recorder := ownerActionHealthRecorderFor(t, handler, tc.capability, tc.service, authz.OwnerActionHealthWriteScope)
			if recorder.Code != tc.want {
				t.Fatalf("status = %d, want %d, body = %s", recorder.Code, tc.want, recorder.Body.String())
			}
			if persisted := store.auditAction != ""; persisted != (tc.want == http.StatusOK) {
				t.Fatalf("persisted = %v for status %d", persisted, tc.want)
			}
		})
	}
}

// WS18-S2 and WS19-S9: the generic and tenant health lanes refuse every owner-action capability, the two room canvas rows
// included, whichever service or tenant attests it and whatever the authorizer would admit.
func TestGenericAndTenantLanesRefuseEveryOwnerActionCapability(t *testing.T) {
	for capabilityID := range ownerActionHealthAttesters {
		t.Run(capabilityID, func(t *testing.T) {
			store := &fakeAvailabilityStore{
				row:     &registry.CapabilityRow{ID: capabilityID, OrgID: "global", Version: "1", Enabled: true, RiskLevel: models.RiskLow},
				updated: true,
			}
			generic := availabilityAttestationRecorder(t, store, capabilityID, "global", authz.ExecutionCoreServiceID, authz.GlobalHealthWriteScope)
			if generic.Code != http.StatusForbidden {
				t.Fatalf("generic lane status = %d, body = %s", generic.Code, generic.Body.String())
			}
			tenant := availabilityAttestationRecorder(t, store, capabilityID, "org-a", "health-a", authz.HealthWriteScope)
			if tenant.Code != http.StatusForbidden {
				t.Fatalf("tenant lane status = %d, body = %s", tenant.Code, tenant.Body.String())
			}
			if store.auditAction != "" {
				t.Fatalf("a generic or tenant health lane reached persistence: %q", store.auditAction)
			}
		})
	}
}
