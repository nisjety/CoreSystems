package attestation_test

import (
	"bytes"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/attestation"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/integration"
)

func TestSharedIntegrationFixtureMatchesPrepareSendAndSigner(t *testing.T) {
	type fixtureSendRequest struct {
		OrgID             string   `json:"org_id"`
		ActorUserID       string   `json:"actor_user_id"`
		Provider          string   `json:"provider"`
		ProviderKey       string   `json:"provider_key"`
		ConnectionID      string   `json:"connection_id"`
		ProviderThreadID  string   `json:"provider_thread_id"`
		BodyText          string   `json:"body_text"`
		BodyHTML          string   `json:"body_html"`
		Subject           string   `json:"subject"`
		To                []string `json:"to"`
		AuthorizationKind string   `json:"authorization_kind"`
		AuthorizationID   string   `json:"authorization_id"`
		ApprovalID        string   `json:"approval_id"`
		ActionID          string   `json:"action_id"`
		IdempotencyKey    string   `json:"idempotency_key"`
	}
	type sharedFixture struct {
		PrivateKeySeedBase64 string             `json:"private_key_seed_base64"`
		Header               map[string]string  `json:"header"`
		Claims               attestation.Claims `json:"claims"`
		SendRequest          fixtureSendRequest `json:"send_request"`
		CanonicalPayloadJSON string             `json:"canonical_payload_json"`
		PayloadSHA256        string             `json:"payload_sha256"`
		CompactJWS           string             `json:"compact_jws"`
	}
	fixturePath := filepath.Join("..", "..", "..", "..", "..", "Ingestion Plane", "integration-corev2", "testdata", "provider_write_attestation_v1.json")
	contents, err := os.ReadFile(fixturePath)
	if err != nil {
		t.Fatal(err)
	}
	var fixture sharedFixture
	if err := json.Unmarshal(contents, &fixture); err != nil {
		t.Fatal(err)
	}
	provider := fixture.SendRequest.Provider
	if provider == "" {
		provider = fixture.SendRequest.ProviderKey
	}
	if provider == "" || fixture.SendRequest.OrgID == "" {
		t.Fatal("shared fixture does not contain a reproducible send_request")
	}
	request := integration.SendRequest{
		OrgID: fixture.SendRequest.OrgID, ActorUserID: fixture.SendRequest.ActorUserID,
		Provider: provider, ConnectionID: fixture.SendRequest.ConnectionID,
		ProviderThreadID: fixture.SendRequest.ProviderThreadID,
		BodyText:         fixture.SendRequest.BodyText, BodyHTML: fixture.SendRequest.BodyHTML,
		Subject: fixture.SendRequest.Subject, To: fixture.SendRequest.To,
	}
	prepared, err := integration.PrepareSend(request)
	if err != nil {
		t.Fatal(err)
	}
	canonical := struct {
		OrgID        string         `json:"org_id"`
		ConnectionID string         `json:"connection_id"`
		ProviderKey  string         `json:"provider_key"`
		Operation    string         `json:"operation"`
		Params       map[string]any `json:"params"`
		Body         map[string]any `json:"body"`
	}{
		OrgID: request.OrgID, ConnectionID: request.ConnectionID, ProviderKey: request.Provider,
		Operation: prepared.Operation, Params: prepared.Params, Body: prepared.Body,
	}
	canonicalJSON, err := json.Marshal(canonical)
	if err != nil {
		t.Fatal(err)
	}
	if string(canonicalJSON) != fixture.CanonicalPayloadJSON || prepared.PayloadSHA256 != fixture.PayloadSHA256 || prepared.PayloadSHA256 != fixture.Claims.PayloadSHA256 {
		t.Fatalf("PrepareSend parity failed: canonical=%s digest=%s", canonicalJSON, prepared.PayloadSHA256)
	}
	seed, err := base64.StdEncoding.Strict().DecodeString(fixture.PrivateKeySeedBase64)
	if err != nil || len(seed) != ed25519.SeedSize {
		t.Fatalf("fixture seed = %d bytes, %v", len(seed), err)
	}
	jtiBytes, err := base64.RawURLEncoding.DecodeString(fixture.Claims.JWTID)
	if err != nil || len(jtiBytes) != 16 {
		t.Fatalf("fixture jti must decode to 16 bytes: %d, %v", len(jtiBytes), err)
	}
	ttl := time.Duration(fixture.Claims.ExpiresAt-fixture.Claims.IssuedAt) * time.Second
	signer, err := attestation.NewSigner(attestation.Config{
		PrivateKey: ed25519.NewKeyFromSeed(seed), KeyID: fixture.Header["kid"],
		Issuer: attestation.IssuerConversationCore, Audience: attestation.AudienceIntegrationCore,
		Presenter: attestation.PresenterConversationCore, TTL: ttl,
		Now:    func() time.Time { return time.Unix(fixture.Claims.IssuedAt, 0).UTC() },
		Random: bytes.NewReader(jtiBytes),
	})
	if err != nil {
		t.Fatal(err)
	}
	compact, err := signer.Sign(attestation.Authorization{
		AuthorizationKind: fixture.SendRequest.AuthorizationKind,
		AuthorizationID:   fixture.SendRequest.AuthorizationID, ApprovalID: fixture.SendRequest.ApprovalID,
		ActionID: fixture.SendRequest.ActionID, OrgID: fixture.SendRequest.OrgID,
		ConnectionID: fixture.SendRequest.ConnectionID, ProviderKey: provider,
		Operation: prepared.Operation, ActorID: fixture.SendRequest.ActorUserID,
		PayloadSHA256: prepared.PayloadSHA256, IdempotencyKey: fixture.SendRequest.IdempotencyKey,
	})
	if err != nil {
		t.Fatal(err)
	}
	if compact != fixture.CompactJWS {
		t.Fatalf("shared compact JWS mismatch\n got: %s\nwant: %s", compact, fixture.CompactJWS)
	}
}
