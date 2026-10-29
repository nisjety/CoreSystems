package users

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"
)

func TestValidateAuthErasureReceiptFailsClosed(t *testing.T) {
	tests := []struct {
		name    string
		payload string
		wantErr string
	}{
		{name: "success", payload: `{"success":true,"user_id":"u-1","deleted_records":{"sessions":["do-not-retain"]}}`},
		{name: "false success", payload: `{"success":false,"user_id":"u-1","error":"database failure"}`, wantErr: "reported failure"},
		{name: "missing success", payload: `{"user_id":"u-1"}`, wantErr: "success must be true"},
		{name: "wrong user", payload: `{"success":true,"user_id":"u-2"}`, wantErr: "unexpected user"},
		{name: "malformed", payload: `{"success":`, wantErr: "decode"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := validateAuthErasureReceipt([]byte(tt.payload), "u-1")
			if tt.wantErr == "" && err != nil {
				t.Fatalf("valid receipt rejected: %v", err)
			}
			if tt.wantErr != "" && (err == nil || !strings.Contains(err.Error(), tt.wantErr)) {
				t.Fatalf("error = %v, want substring %q", err, tt.wantErr)
			}
		})
	}
}

func TestErasureOperationIDIsStableAndModeBound(t *testing.T) {
	hardA := erasureOperationID("u-1", ErasureModeHardDelete)
	hardB := erasureOperationID("u-1", ErasureModeHardDelete)
	anonymize := erasureOperationID("u-1", ErasureModeAnonymize)
	if hardA == "" || hardA != hardB {
		t.Fatalf("hard-delete operation ID is not stable: %q / %q", hardA, hardB)
	}
	if hardA == anonymize {
		t.Fatal("operation ID must bind the erasure mode")
	}
}

func TestValidateErasureOperationRejectsUnverifiedAuthority(t *testing.T) {
	base := ErasureOperation{UserID: "u-1", ActorID: "u-1", ActorRole: "self", OrgID: "org-1", Mode: ErasureModeHardDelete}
	if err := validateErasureOperation(base); err != nil {
		t.Fatalf("valid operation rejected: %v", err)
	}
	for name, mutate := range map[string]func(*ErasureOperation){
		"missing identity": func(operation *ErasureOperation) { operation.OrgID = "" },
		"unverified role":  func(operation *ErasureOperation) { operation.ActorRole = "member" },
		"invalid mode":     func(operation *ErasureOperation) { operation.Mode = "purge_everything" },
	} {
		t.Run(name, func(t *testing.T) {
			operation := base
			mutate(&operation)
			if err := validateErasureOperation(operation); err == nil {
				t.Fatal("invalid operation was accepted")
			}
		})
	}
}

func TestErasureRetryDelayIsBounded(t *testing.T) {
	if got := erasureRetryDelay(0); got != time.Second {
		t.Fatalf("first delay = %v, want 1s", got)
	}
	if got := erasureRetryDelay(100); got != 5*time.Minute {
		t.Fatalf("capped delay = %v, want 5m", got)
	}
}

func TestErasureSagaWiringFailsClosedWhenUnavailable(t *testing.T) {
	svc := &Service{}
	svc.StartErasureSaga()
	svc.CloseErasureSaga()
	if _, err := svc.ResolveErasureAuditOrg(t.Context(), "u-1", ""); err == nil {
		t.Fatal("audit org resolution without a repository must fail closed")
	}
	var nilService *Service
	nilService.CloseErasureSaga()
}

type fakeAuthErasureExecutor struct {
	execute func(context.Context, ErasureMode, string) ([]byte, error)
	calls   int
}

func (f *fakeAuthErasureExecutor) Execute(ctx context.Context, mode ErasureMode, userID string) ([]byte, error) {
	f.calls++
	if f.execute == nil {
		return nil, errors.New("fake auth erasure is not configured")
	}
	return f.execute(ctx, mode, userID)
}

type fakeErasureFanoutPublisher struct {
	failures   int
	failOnCall int
	calls      int
	eventIDs   []string
	payloads   [][]byte
}

func (f *fakeErasureFanoutPublisher) PublishGDPRErasure(_ context.Context, eventID string, payload []byte) error {
	f.calls++
	f.eventIDs = append(f.eventIDs, eventID)
	f.payloads = append(f.payloads, append([]byte(nil), payload...))
	if f.failOnCall > 0 && f.calls == f.failOnCall {
		return errors.New("shared NATS PubAck unavailable")
	}
	if f.calls <= f.failures {
		return errors.New("shared NATS unavailable")
	}
	return nil
}
