package activities

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

func validScheduledRunExecutionIntent() ScheduledRunExecutionIntent {
	return ScheduledRunExecutionIntent{
		OrgID: "org-1", SpaceRef: "space-1", SubjectID: "user-1", ThreadID: "thread-1",
		ScheduleID: "schedule-1", FireKey: "2026-08-15T10:00:00Z", RunID: "task-1",
		TemplateDigest: "sha256:" + strings.Repeat("a", 64), IdempotencyKey: "schedule-1:2026-08-15T10:00:00Z",
	}
}

func signedScheduledRunExecutionToken(t *testing.T, private ed25519.PrivateKey, keyID string, intent ScheduledRunExecutionIntent) string {
	t.Helper()
	now := time.Now().UTC()
	decision := scheduledRunExecutionDecision{
		DecisionRef: "decision-1", OrgID: intent.OrgID, SpaceRef: intent.SpaceRef, SubjectID: intent.SubjectID,
		ServiceAudience: controlScheduledRunExecutionAudience, ActionID: controlScheduledRunExecutionAction,
		ActionSchemaHash: controlScheduledRunExecutionSchema, IdempotencyKey: intent.IdempotencyKey,
		RecipientAudienceRef: "audience-1", RecipientAudienceHash: "sha256:audience",
		PrivacyPolicyRef: "privacy-1", ResourceAuthorizationRef: "resource-1",
		AuthorityRevision: 7, MembershipRevision: 4, PrivacyRevision: 5,
		RecipientAudienceRevision: 2, EntitlementRevision: 3, Permissions: []string{"schedule:execute"},
		IssuedAt: now.Add(-time.Second), ExpiresAt: now.Add(time.Minute), Nonce: "nonce-1",
	}
	decision.PayloadDigest = scheduledRunExecutionPayloadDigest(decision, intent)
	payload, err := json.Marshal(decision)
	if err != nil {
		t.Fatal(err)
	}
	signed := controlScheduledRunDecisionVersion + "." +
		base64.RawURLEncoding.EncodeToString([]byte(keyID)) + "." +
		base64.RawURLEncoding.EncodeToString(payload)
	return signed + "." + base64.RawURLEncoding.EncodeToString(ed25519.Sign(private, []byte(signed)))
}

func TestControlScheduledRunExecutionAuthorizerSendsExactWorkerIdentityAndBindsThread(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	intent := validScheduledRunExecutionIntent()
	token := signedScheduledRunExecutionToken(t, private, "key-1", intent)
	calls := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/internal/spaces/scheduled-run-execution-decision" {
			t.Fatalf("path = %q", r.URL.Path)
		}
		if r.Header.Get("X-Service-Id") != controlScheduledRunExecutorPrincipal ||
			r.Header.Get("X-Service-Token") != "executor-token" {
			t.Fatalf("Control worker identity was not sent")
		}
		var body struct {
			Intent map[string]string `json:"intent"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Fatal(err)
		}
		calls++
		if calls == 1 && (body.Intent["thread_id"] != intent.ThreadID || body.Intent["task_id"] != intent.RunID) {
			t.Fatalf("execution intent not bound to prepared thread/run: %#v", body.Intent)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]string{"token": token}})
	}))
	defer server.Close()

	authorizer, err := NewControlScheduledRunExecutionAuthorizer(
		server.URL, "executor-token", "key-1", base64.RawURLEncoding.EncodeToString(public), server.Client(),
	)
	if err != nil {
		t.Fatal(err)
	}
	got, err := authorizer.AuthorizeScheduledRunExecution(context.Background(), intent)
	if err != nil {
		t.Fatalf("AuthorizeScheduledRunExecution: %v", err)
	}
	if got != token {
		t.Fatal("execution token was not returned unchanged for the direct Session Core hop")
	}

	changed := intent
	changed.ThreadID = "thread-2"
	if _, err := authorizer.AuthorizeScheduledRunExecution(context.Background(), changed); err == nil {
		t.Fatal("execution decision for another prepared thread was accepted")
	}
}

func TestControlScheduledRunExecutionAuthorizerRejectsDifferentRunOrIdempotencyBindings(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	baseIntent := validScheduledRunExecutionIntent()
	token := signedScheduledRunExecutionToken(t, private, "key-1", baseIntent)
	fake := &recordingServer{
		Token: token,
		Path:  "/api/v1/internal/spaces/scheduled-run-execution-decision",
	}
	server := fake.start(t)
	defer server.Close()

	authorizer, err := NewControlScheduledRunExecutionAuthorizer(
		server.URL, "executor-token", "key-1", base64.RawURLEncoding.EncodeToString(public), server.Client(),
	)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := authorizer.AuthorizeScheduledRunExecution(context.Background(), baseIntent); err != nil {
		t.Fatalf("valid scheduled-run execution decision denied: %v", err)
	}

	changedRun := baseIntent
	changedRun.RunID = "task-2"
	if _, err := authorizer.AuthorizeScheduledRunExecution(context.Background(), changedRun); err == nil {
		t.Fatal("execution decision was reused with a different run_id")
	}

	changedIdempotency := baseIntent
	changedIdempotency.IdempotencyKey = "schedule-1:2026-08-15T10:10:00Z"
	if _, err := authorizer.AuthorizeScheduledRunExecution(context.Background(), changedIdempotency); err == nil {
		t.Fatal("execution decision was reused with a different idempotency key")
	}
}

type recordingServer struct {
	Token string
	Path  string
}

func (s *recordingServer) start(t *testing.T) *httptest.Server {
	t.Helper()
	return httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != s.Path {
			t.Fatalf("unexpected path %q", r.URL.Path)
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"data": map[string]string{"token": s.Token}})
	}))
}
