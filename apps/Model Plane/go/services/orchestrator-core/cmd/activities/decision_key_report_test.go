package activities

import (
	"bytes"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"log/slog"
	"strings"
	"testing"
	"time"
)

// F11: a scheduled-step decision signed under a key id this service does not trust
// is still refused with the same generic error, but the operator now gets an error
// log naming both ids. The key itself is never logged.
func TestScheduledStepAuthorizerLogsBothKeyIDsWhenTokenKeyIsUntrusted(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	var logs bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(&logs, nil)))
	defer slog.SetDefault(previous)

	now := time.Date(2026, 10, 31, 0, 0, 0, 0, time.UTC)
	intent := validScheduledStepExecutionIntent()
	authorizer := &ControlScheduledStepAuthorizer{keyID: "stale-key-1", public: public}
	token := signedScheduledStepToken(t, "ledger-key-2-step", private, intent, now)
	verifyErr := authorizer.verify(token, intent, now)
	if verifyErr == nil || !strings.Contains(verifyErr.Error(), "untrusted Control scheduled-step decision key") {
		t.Fatalf("refusal changed: %v", verifyErr)
	}
	out := logs.String()
	for _, want := range []string{`"level":"ERROR"`, `"token_key_id":"ledger-key-2-step"`, `"configured_key_id":"stale-key-1"`, `"verifier":"scheduled-step"`} {
		if !strings.Contains(out, want) {
			t.Fatalf("log missing %s: %s", want, out)
		}
	}
	if strings.Contains(out, base64.RawURLEncoding.EncodeToString(public)) {
		t.Fatalf("the public key leaked into the log: %s", out)
	}
}

func TestScheduledRunAuthorizerLogsBothKeyIDsWhenTokenKeyIsUntrusted(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	var logs bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(&logs, nil)))
	defer slog.SetDefault(previous)

	intent := validScheduledRunExecutionIntent()
	authorizer := &ControlScheduledRunExecutionAuthorizer{keyID: "stale-key-1", public: public}
	token := signedScheduledRunExecutionToken(t, private, "ledger-key-2-run", intent)
	verifyErr := authorizer.verify(token, intent, time.Now().UTC())
	if verifyErr == nil || !strings.Contains(verifyErr.Error(), "untrusted Control scheduled-run execution decision key") {
		t.Fatalf("refusal changed: %v", verifyErr)
	}
	out := logs.String()
	for _, want := range []string{`"level":"ERROR"`, `"token_key_id":"ledger-key-2-run"`, `"configured_key_id":"stale-key-1"`, `"verifier":"scheduled-run"`} {
		if !strings.Contains(out, want) {
			t.Fatalf("log missing %s: %s", want, out)
		}
	}
}

// A public key pasted as standard base64 (with + and /) must not silently disable
// the lane: it decodes to the same key.
func TestScheduledAuthorizersAcceptStandardBase64PublicKey(t *testing.T) {
	var public ed25519.PublicKey
	for {
		var err error
		public, _, err = ed25519.GenerateKey(rand.Reader)
		if err != nil {
			t.Fatal(err)
		}
		if strings.ContainsAny(base64.StdEncoding.EncodeToString(public), "+/") {
			break
		}
	}
	encoded := base64.StdEncoding.EncodeToString(public)
	if _, err := NewControlScheduledStepAuthorizer("http://control", "token", "key-1", encoded, nil); err != nil {
		t.Fatalf("step authorizer refused a standard-base64 key: %v", err)
	}
	if _, err := NewControlScheduledRunExecutionAuthorizer("http://control", "token", "key-1", encoded, nil); err != nil {
		t.Fatalf("run authorizer refused a standard-base64 key: %v", err)
	}
	if _, err := NewControlScheduledStepAuthorizer("http://control", "token", "key-1", "not a key", nil); err == nil {
		t.Fatal("garbage key accepted")
	}
}
