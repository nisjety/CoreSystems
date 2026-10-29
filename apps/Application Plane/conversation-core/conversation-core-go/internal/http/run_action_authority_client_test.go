package http

import (
	"context"
	"encoding/json"
	stdhttp "net/http"
	"net/http/httptest"
	"testing"
)

const controlRunActionAuthorityTestToken = "control-run-action-authority-test-token-32"

func TestControlRunActionAuthorityValidatorUsesDedicatedPrincipalAndExactDecisionFacts(t *testing.T) {
	decision := agentTicketTestDecision(agentTicketCreateBody{
		RunID: "run_1", IdempotencyKey: "ticket-agent-1",
	})
	server := httptest.NewServer(stdhttp.HandlerFunc(func(w stdhttp.ResponseWriter, request *stdhttp.Request) {
		if request.Method != stdhttp.MethodPost || request.URL.Path != "/api/v1/internal/spaces/run-action-authority-check" {
			t.Fatalf("request = %s %s", request.Method, request.URL.Path)
		}
		if request.Header.Get("X-Service-Id") != "conversation-core" || request.Header.Get("X-Service-Token") != controlRunActionAuthorityTestToken {
			t.Fatalf("service headers = %q/%q", request.Header.Get("X-Service-Id"), request.Header.Get("X-Service-Token"))
		}
		var body struct {
			RunID                     string `json:"run_id"`
			OrgID                     string `json:"org_id"`
			SpaceRef                  string `json:"space_ref"`
			SubjectID                 string `json:"subject_id"`
			RecipientAudienceRef      string `json:"recipient_audience_ref"`
			RecipientAudienceHash     string `json:"recipient_audience_hash"`
			RecipientAudienceRevision int64  `json:"recipient_audience_revision"`
			PrivacyPolicyRef          string `json:"privacy_policy_ref"`
			AuthorityRevision         int64  `json:"authority_revision"`
			ActionID                  string `json:"action_id"`
		}
		if err := json.NewDecoder(request.Body).Decode(&body); err != nil {
			t.Fatalf("Decode() error = %v", err)
		}
		if body.RunID != decision.RunID || body.OrgID != decision.OrgID || body.SpaceRef != decision.SpaceRef ||
			body.SubjectID != decision.SubjectID || body.RecipientAudienceRef != decision.RecipientAudienceRef ||
			body.RecipientAudienceHash != decision.RecipientAudienceHash || body.RecipientAudienceRevision != decision.RecipientAudienceRevision ||
			body.PrivacyPolicyRef != decision.PrivacyPolicyRef || body.AuthorityRevision != decision.AuthorityRevision ||
			body.ActionID != decision.ActionID {
			t.Fatalf("authority check body = %#v, decision = %#v", body, decision)
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"data":{"authorized":true}}`))
	}))
	defer server.Close()

	validator, err := NewControlRunActionAuthorityValidator(server.URL, controlRunActionAuthorityTestToken, true)
	if err != nil {
		t.Fatalf("NewControlRunActionAuthorityValidator() error = %v", err)
	}
	if err := validator.ValidateRunActionAuthority(context.Background(), decision); err != nil {
		t.Fatalf("ValidateRunActionAuthority() error = %v", err)
	}
}

func TestControlRunActionAuthorityValidatorMapsControlDenialToOwnerDenial(t *testing.T) {
	server := httptest.NewServer(stdhttp.HandlerFunc(func(w stdhttp.ResponseWriter, _ *stdhttp.Request) {
		w.WriteHeader(stdhttp.StatusForbidden)
	}))
	defer server.Close()
	validator, err := NewControlRunActionAuthorityValidator(server.URL, controlRunActionAuthorityTestToken, true)
	if err != nil {
		t.Fatalf("NewControlRunActionAuthorityValidator() error = %v", err)
	}
	if err := validator.ValidateRunActionAuthority(context.Background(), agentTicketTestDecision(agentTicketCreateBody{RunID: "run_1", IdempotencyKey: "ticket-agent-1"})); err != ErrRunActionAuthorityDenied {
		t.Fatalf("ValidateRunActionAuthority() error = %v, want ErrRunActionAuthorityDenied", err)
	}
}

func TestControlRunActionAuthorityValidatorRejectsPlaintextWithoutExplicitLoopbackOptIn(t *testing.T) {
	if _, err := NewControlRunActionAuthorityValidator("http://127.0.0.1:8080", controlRunActionAuthorityTestToken, false); err == nil {
		t.Fatal("NewControlRunActionAuthorityValidator() error = nil, want plaintext rejection")
	}
	if _, err := NewControlRunActionAuthorityValidator("http://user-core:8080", controlRunActionAuthorityTestToken, true); err == nil {
		t.Fatal("NewControlRunActionAuthorityValidator() error = nil, want non-loopback plaintext rejection")
	}
	if _, err := NewControlRunActionAuthorityValidator("https://user-core:8443", controlRunActionAuthorityTestToken, false); err != nil {
		t.Fatalf("NewControlRunActionAuthorityValidator() HTTPS error = %v", err)
	}
}
