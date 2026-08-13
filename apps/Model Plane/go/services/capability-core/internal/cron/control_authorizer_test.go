package cron

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

func validControlFireIntent() FireIntent {
	return FireIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "cron-1",
		FireKey: "2026-08-13T10:00:00Z", TemplateDigest: "sha256:" + strings.Repeat("a", 64),
		IdempotencyKey: "cron-1:2026-08-13T10:00:00Z",
	}
}

func validControlCreateIntent() CreateIntent {
	return CreateIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ScheduleID: "cron-1",
		TemplateDigest: "sha256:" + strings.Repeat("b", 64), IdempotencyKey: "create-cron-1",
	}
}

func signedControlFireToken(t *testing.T, private ed25519.PrivateKey, keyID string, intent FireIntent) string {
	t.Helper()
	now := time.Now().UTC()
	decision := controlDecision{
		DecisionRef: "decision-1", OrgID: intent.OrgID, SpaceRef: intent.SpaceRef, SubjectID: intent.SubjectID,
		ServiceAudience: controlFireAudience, ActionID: controlFireAction, ActionSchemaHash: controlFireSchema,
		IdempotencyKey: intent.IdempotencyKey, RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ResourceAuthorizationRef: "resource-1", AuthorityRevision: 7,
		MembershipRevision: 4, PrivacyRevision: 5, RecipientAudienceRevision: 2, EntitlementRevision: 3,
		Permissions: []string{"cron:fire"}, IssuedAt: now.Add(-time.Second), ExpiresAt: now.Add(time.Minute),
	}
	decision.PayloadDigest = expectedFirePayloadDigest(decision, intent)
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	signed := controlDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
}

func signedControlCreateToken(t *testing.T, private ed25519.PrivateKey, keyID string, intent CreateIntent) string {
	t.Helper()
	now := time.Now().UTC()
	decision := controlDecision{
		DecisionRef: "decision-1", OrgID: intent.OrgID, SpaceRef: intent.SpaceRef, SubjectID: intent.SubjectID,
		ServiceAudience: controlCreateAudience, ActionID: controlCreateAction, ActionSchemaHash: controlCreateSchema,
		IdempotencyKey: intent.IdempotencyKey, RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ResourceAuthorizationRef: "resource-1", AuthorityRevision: 7,
		MembershipRevision: 4, PrivacyRevision: 5, RecipientAudienceRevision: 2, EntitlementRevision: 3,
		Permissions: []string{"cron:create"}, IssuedAt: now.Add(-time.Second), ExpiresAt: now.Add(time.Minute),
	}
	decision.PayloadDigest = expectedCreatePayloadDigest(decision, intent)
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	signed := controlDecisionVersion + "." + base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." + base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
}

func TestControlFireAuthorizerAcceptsOnlyFreshSignedBoundDecision(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	intent := validControlFireIntent()
	token := signedControlFireToken(t, private, "key-1", intent)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("X-Service-Token") != "scheduler-token" {
			t.Fatal("scheduler credential was not sent")
		}
		if r.URL.Path != "/api/v1/internal/spaces/schedule-fire-decision" {
			t.Fatalf("unexpected Control endpoint %q", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]string{"token": token}})
	}))
	defer server.Close()
	authorizer, err := NewControlFireAuthorizer(server.URL, "scheduler-token", "key-1", base64.RawURLEncoding.EncodeToString(public), server.Client())
	if err != nil {
		t.Fatal(err)
	}
	if err := authorizer.AuthorizeFire(context.Background(), intent); err != nil {
		t.Fatalf("valid fresh signed decision denied: %v", err)
	}
}

func TestControlFireAuthorizerRejectsDecisionForAnotherFire(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	intent := validControlFireIntent()
	other := intent
	other.FireKey = "2026-08-13T10:05:00Z"
	other.IdempotencyKey = "cron-1:2026-08-13T10:05:00Z"
	token := signedControlFireToken(t, private, "key-1", other)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]string{"token": token}})
	}))
	defer server.Close()
	authorizer, err := NewControlFireAuthorizer(server.URL, "scheduler-token", "key-1", base64.RawURLEncoding.EncodeToString(public), server.Client())
	if err != nil {
		t.Fatal(err)
	}
	if err := authorizer.AuthorizeFire(context.Background(), intent); err == nil {
		t.Fatal("decision for another deterministic fire was accepted")
	}
}

func TestControlDecisionVerifierAcceptsOnlyTemplateBoundScheduleCreate(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	intent := validControlCreateIntent()
	verifier, err := NewControlDecisionVerifier("key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	token := signedControlCreateToken(t, private, "key-1", intent)
	if _, err := verifier.VerifyScheduleCreate(token, intent, time.Now().UTC()); err != nil {
		t.Fatalf("valid create decision denied: %v", err)
	}
	changed := intent
	changed.TemplateDigest = "sha256:" + strings.Repeat("c", 64)
	if _, err := verifier.VerifyScheduleCreate(token, changed, time.Now().UTC()); err == nil {
		t.Fatal("create decision was reused with another template")
	}
}
