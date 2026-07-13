package integration

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/attestation"
)

type recordingAttestor struct {
	authorization attestation.Authorization
	compact       string
	err           error
	calls         int
}

func (r *recordingAttestor) Sign(authorization attestation.Authorization) (string, error) {
	r.calls++
	r.authorization = authorization
	return r.compact, r.err
}

func TestPrepareSendUsesStableCanonicalEffectDigest(t *testing.T) {
	request := SendRequest{
		OrgID: " org-1 ", Provider: " whatsapp ", ConnectionID: " conn-1 ",
		ProviderThreadID: "phone-1:user-1", BodyText: "hello",
	}
	first, err := PrepareSend(request)
	if err != nil {
		t.Fatal(err)
	}
	second, err := PrepareSend(request)
	if err != nil {
		t.Fatal(err)
	}
	if first.PayloadSHA256 != second.PayloadSHA256 || len(first.PayloadSHA256) != 64 {
		t.Fatalf("digest is not stable SHA-256: %q / %q", first.PayloadSHA256, second.PayloadSHA256)
	}
	const expected = "12cfb11e5a2bd64616ba8f0f9d1f032176f337c4e38979a3ae18fb1f417f728c"
	if first.PayloadSHA256 != expected {
		t.Fatalf("canonical digest changed: got %s want %s", first.PayloadSHA256, expected)
	}
	if first.Operation != "whatsapp.messages.send" {
		t.Fatalf("operation = %q", first.Operation)
	}
}

func TestSendFailsClosedWithoutAttestorOrExactPreparedDigestBeforeHTTP(t *testing.T) {
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { requests++ }))
	defer server.Close()
	base := SendRequest{
		OrgID: "org-1", Provider: "slack", ConnectionID: "conn-1", ProviderThreadID: "C123",
		BodyText: "hello", AuthorizationKind: attestation.AuthorizationHumanIntent,
		AuthorizationID: "outintent-1", ActionID: "outintent-1", ActorUserID: "user-1",
		IdempotencyKey: "conversation:reply-1",
	}
	prepared, err := PrepareSend(base)
	if err != nil {
		t.Fatal(err)
	}
	base.PayloadSHA256 = prepared.PayloadSHA256

	client := NewClient(server.URL, "internal", WithServicePrincipal(server.URL, "conversation-core", "credential"))
	_, err = client.Send(t.Context(), base)
	if err == nil || ErrorCode(err) != "attestation_not_configured" {
		t.Fatalf("nil attestor error = %v", err)
	}

	attestor := &recordingAttestor{compact: "signed-proof"}
	client = NewClient(server.URL, "internal", WithServicePrincipal(server.URL, "conversation-core", "credential"), WithWriteAttestor(attestor))
	base.PayloadSHA256 = strings.Repeat("0", 64)
	_, err = client.Send(t.Context(), base)
	if err == nil || ErrorCode(err) != "payload_binding_mismatch" {
		t.Fatalf("changed digest error = %v", err)
	}
	if attestor.calls != 0 || requests != 0 {
		t.Fatalf("attestor/http calls = %d/%d, want 0/0", attestor.calls, requests)
	}
}

func TestSendPlacesOnlyEffectBoundWriteAttestationInActionBody(t *testing.T) {
	authServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"token":"tenant-token","expiresAt":"2099-01-01T00:00:00Z"}`))
	}))
	defer authServer.Close()
	var actionBody map[string]any
	integrationServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if err := json.NewDecoder(r.Body).Decode(&actionBody); err != nil {
			t.Fatal(err)
		}
		_, _ = w.Write([]byte(`{"success":true,"data":{"action":{"result":{"ts":"42.1"}}}}`))
	}))
	defer integrationServer.Close()

	attestor := &recordingAttestor{compact: "signed-proof"}
	client := NewClient(integrationServer.URL, "internal",
		WithServicePrincipal(authServer.URL, "conversation-core", "credential"),
		WithWriteAttestor(attestor),
	)
	request := SendRequest{
		OrgID: "org-1", Provider: "slack", ConnectionID: "conn-1", ProviderThreadID: "C123",
		BodyText: "hello", AuthorizationKind: attestation.AuthorizationHumanApprovedAIAction,
		AuthorizationID: "outintent-1", ApprovalID: "action-1", ActionID: "action-1",
		ActorUserID: "reviewer-1", IdempotencyKey: "conversation-ai:action-1",
	}
	prepared, err := PrepareSend(request)
	if err != nil {
		t.Fatal(err)
	}
	request.PayloadSHA256 = prepared.PayloadSHA256
	if _, err := client.Send(context.Background(), request); err != nil {
		t.Fatal(err)
	}
	if actionBody["writeAttestation"] != "signed-proof" {
		t.Fatalf("writeAttestation = %#v", actionBody["writeAttestation"])
	}
	if _, exists := actionBody["approvalId"]; exists {
		t.Fatalf("legacy approvalId leaked into action body: %#v", actionBody)
	}
	if attestor.calls != 1 || attestor.authorization.PayloadSHA256 != prepared.PayloadSHA256 || attestor.authorization.ActorID != "reviewer-1" {
		t.Fatalf("signed authorization = %#v calls=%d", attestor.authorization, attestor.calls)
	}
}
