package activities

import (
	"context"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"go.temporal.io/sdk/temporal"
	"google.golang.org/grpc"

	"github.com/triodelab/model-plane/services/orchestrator-core/internal/grpcclient"
)

// Control answers a typed Space refusal (entitlement off, role floor, kind
// unsupported) with the 403 its denial vocabulary assigns, and keeps 503 for
// "could not answer". A Temporal activity must not retry the first and must keep
// retrying the second, a 5xx, a rate limit and a dead socket.

const (
	refusalBody     = `{"error":"personal Space scheduled run authority unavailable","code":"space_entitlement_off","resolvable_by":"org_admin","details":{"entitlement":"schedule_run"}}`
	unavailableBody = `{"error":"personal Space scheduled run authority unavailable","code":"space_authority_unavailable","resolvable_by":"retry","details":{}}`
)

var controlStatusCases = []struct {
	name        string
	status      int
	body        string
	wantRefused bool
	wantCode    string
}{
	{"typed refusal", http.StatusForbidden, refusalBody, true, "space_entitlement_off"},
	{"plain membership refusal", http.StatusForbidden, `{"error":"current personal Space authority required"}`, true, ""},
	{"malformed intent", http.StatusBadRequest, `{"error":"valid scheduled run intent is required"}`, true, ""},
	{"refusal with a non-JSON body", http.StatusForbidden, `forbidden`, true, ""},
	{"authority unavailable", http.StatusServiceUnavailable, unavailableBody, false, ""},
	{"plain outage", http.StatusServiceUnavailable, `{"error":"Space authority repository unavailable"}`, false, ""},
	{"server error", http.StatusInternalServerError, `{"error":"boom"}`, false, ""},
	{"gateway timeout", http.StatusGatewayTimeout, `timeout`, false, ""},
	{"credential not yet in place", http.StatusUnauthorized, `{"error":"unauthorized"}`, false, ""},
	{"rate limited", http.StatusTooManyRequests, `{"error":"slow down"}`, false, ""},
	{"request timeout", http.StatusRequestTimeout, `{"error":"slow"}`, false, ""},
}

func controlStatusServer(t *testing.T, status int, body string) *httptest.Server {
	t.Helper()
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_, _ = w.Write([]byte(body))
	}))
	t.Cleanup(server.Close)
	return server
}

func publicKeyBase64(t *testing.T) string {
	t.Helper()
	public, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	return base64.RawURLEncoding.EncodeToString(public)
}

func assertRefusalClassification(t *testing.T, err error, wantRefused bool, wantCode string) {
	t.Helper()
	if err == nil {
		t.Fatal("a non-200 answer was accepted")
	}
	var refused *ControlRefusedError
	if got := errors.As(err, &refused); got != wantRefused {
		t.Fatalf("refused = %t, want %t (%v)", got, wantRefused, err)
	}
	if wantRefused && refused.Code != wantCode {
		t.Fatalf("code = %q, want %q", refused.Code, wantCode)
	}
}

func TestScheduledRunExecutionAuthorizerSeparatesARefusalFromAnOutage(t *testing.T) {
	for _, tc := range controlStatusCases {
		t.Run(tc.name, func(t *testing.T) {
			server := controlStatusServer(t, tc.status, tc.body)
			authorizer, err := NewControlScheduledRunExecutionAuthorizer(server.URL, "executor-token", "key-1", publicKeyBase64(t), server.Client())
			if err != nil {
				t.Fatal(err)
			}
			_, err = authorizer.AuthorizeScheduledRunExecution(context.Background(), validScheduledRunExecutionIntent())
			assertRefusalClassification(t, err, tc.wantRefused, tc.wantCode)
		})
	}
}

func TestScheduledStepAuthorizerSeparatesARefusalFromAnOutage(t *testing.T) {
	for _, tc := range controlStatusCases {
		t.Run(tc.name, func(t *testing.T) {
			server := controlStatusServer(t, tc.status, tc.body)
			authorizer, err := NewControlScheduledStepAuthorizer(server.URL, "step-token", "key-1", publicKeyBase64(t), server.Client())
			if err != nil {
				t.Fatal(err)
			}
			_, err = authorizer.AuthorizeScheduledStep(context.Background(), validScheduledStepExecutionIntent())
			assertRefusalClassification(t, err, tc.wantRefused, tc.wantCode)
		})
	}
}

func TestControlRefusedErrorKeepsTheOriginalRejectionMessage(t *testing.T) {
	plain := &ControlRefusedError{Operation: "Control scheduled-step authority", Status: http.StatusForbidden}
	if plain.Error() != "Control scheduled-step authority rejected: status 403" {
		t.Fatalf("message drifted: %q", plain.Error())
	}
	typed := &ControlRefusedError{Operation: "Control scheduled-step authority", Status: http.StatusForbidden, Code: "space_entitlement_off"}
	if typed.Error() != "Control scheduled-step authority rejected: status 403 (space_entitlement_off)" {
		t.Fatalf("message drifted: %q", typed.Error())
	}
}

func TestNonRetryableIfRefusedStopsOnlyRefusals(t *testing.T) {
	if nonRetryableIfRefused(nil) != nil {
		t.Fatal("nil became an error")
	}

	refused := fmt.Errorf("wrapped: %w", &ControlRefusedError{Operation: "op", Status: http.StatusForbidden, Code: "space_entitlement_off"})
	var application *temporal.ApplicationError
	if !errors.As(nonRetryableIfRefused(refused), &application) || !application.NonRetryable() {
		t.Fatalf("a refusal must be a non-retryable application error, got %v", nonRetryableIfRefused(refused))
	}
	var cause *ControlRefusedError
	if !errors.As(nonRetryableIfRefused(refused), &cause) {
		t.Fatal("the refusal cause was lost")
	}

	for name, err := range map[string]error{
		"outage":      errors.New("op rejected: status 503"),
		"dead socket": fmt.Errorf("request: %w", errors.New("connection refused")),
	} {
		if got := nonRetryableIfRefused(err); got != err {
			t.Fatalf("%s must be returned unchanged and keep retrying, got %v", name, got)
		}
	}
}

type stubStepAuthorizer struct{ err error }

func (s stubStepAuthorizer) AuthorizeScheduledStep(context.Context, ScheduledStepExecutionIntent) (string, error) {
	return "", s.err
}

type stubRunExecutionAuthorizer struct{ err error }

func (s stubRunExecutionAuthorizer) AuthorizeScheduledRunExecution(context.Context, ScheduledRunExecutionIntent) (string, error) {
	return "", s.err
}

func isNonRetryable(err error) bool {
	var application *temporal.ApplicationError
	return errors.As(err, &application) && application.NonRetryable()
}

func TestAuthorizeScheduledStepActivityDoesNotRetryARefusal(t *testing.T) {
	refusal := &ControlRefusedError{Operation: "Control scheduled-step authority", Status: http.StatusForbidden, Code: "space_entitlement_off"}
	a := &Activities{scheduledStepAuthorizer: stubStepAuthorizer{err: refusal}}
	_, err := a.AuthorizeScheduledStepActivity(context.Background(), validScheduledStepExecutionIntent())
	if !isNonRetryable(err) {
		t.Fatalf("a Control refusal must not be retried, got %v", err)
	}

	a = &Activities{scheduledStepAuthorizer: stubStepAuthorizer{err: errors.New("Control scheduled-step authority rejected: status 503")}}
	_, err = a.AuthorizeScheduledStepActivity(context.Background(), validScheduledStepExecutionIntent())
	if err == nil || isNonRetryable(err) {
		t.Fatalf("a Control outage must keep retrying, got %v", err)
	}
}

func TestStartScheduledRunActivityDoesNotRetryARefusal(t *testing.T) {
	clients := &grpcclient.Clients{SessionCore: &grpc.ClientConn{}}
	args := func(a *Activities) error {
		_, err := a.StartScheduledRunActivity(context.Background(),
			"task-1", "thread-1", "org-1", "space-1", "user-1", "schedule-1", "fire-1",
			"sha256:digest", "key-1", "goal", "execute", "", "", "")
		return err
	}

	refusal := &ControlRefusedError{Operation: "Control scheduled-run execution authority", Status: http.StatusForbidden, Code: "space_entitlement_off"}
	err := args(&Activities{clients: clients, scheduledRunExecutionAuthorizer: stubRunExecutionAuthorizer{err: refusal}})
	if !isNonRetryable(err) {
		t.Fatalf("a Control refusal must not be retried, got %v", err)
	}

	err = args(&Activities{clients: clients, scheduledRunExecutionAuthorizer: stubRunExecutionAuthorizer{err: errors.New("Control scheduled-run execution authority rejected: status 503")}})
	if err == nil || isNonRetryable(err) {
		t.Fatalf("a Control outage must keep retrying, got %v", err)
	}
}
