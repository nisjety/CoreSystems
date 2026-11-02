package cron

import (
	"bytes"
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"log/slog"
	"strings"
	"testing"
)

// F11: a token signed under a key id this service does not trust is still refused
// with the same generic error, but the operator now gets an error log naming both
// ids. The key itself is never logged.
func TestControlDecisionVerifierLogsBothKeyIDsWhenTokenKeyIsUntrusted(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	var logs bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(&logs, nil)))
	defer slog.SetDefault(previous)

	verifier, err := NewControlDecisionVerifier("stale-key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	token := signedControlFireToken(t, private, "ledger-key-2-cron", validControlFireIntent())
	_, verifyErr := verifier.verify(token)
	if verifyErr == nil || !strings.Contains(verifyErr.Error(), "untrusted Control schedule decision key") {
		t.Fatalf("refusal changed: %v", verifyErr)
	}
	out := logs.String()
	for _, want := range []string{`"level":"ERROR"`, `"token_key_id":"ledger-key-2-cron"`, `"configured_key_id":"stale-key-1"`} {
		if !strings.Contains(out, want) {
			t.Fatalf("log missing %s: %s", want, out)
		}
	}
	if strings.Contains(out, base64.RawURLEncoding.EncodeToString(public)) {
		t.Fatalf("the public key leaked into the log: %s", out)
	}

	// The same unknown id again inside the window is rate limited.
	logs.Reset()
	if _, err := verifier.verify(token); err == nil {
		t.Fatal("untrusted key accepted")
	}
	if logs.Len() != 0 {
		t.Fatalf("repeat sighting was not rate limited: %s", logs.String())
	}
}

func TestControlDecisionVerifierTrustedKeyDoesNotLog(t *testing.T) {
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	var logs bytes.Buffer
	previous := slog.Default()
	slog.SetDefault(slog.New(slog.NewJSONHandler(&logs, nil)))
	defer slog.SetDefault(previous)

	verifier, err := NewControlDecisionVerifier("key-1", base64.RawURLEncoding.EncodeToString(public))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := verifier.verify(signedControlFireToken(t, private, "key-1", validControlFireIntent())); err != nil {
		t.Fatalf("trusted token refused: %v", err)
	}
	if logs.Len() != 0 {
		t.Fatalf("a trusted token logged: %s", logs.String())
	}
}

// A public key pasted as standard base64 (with + and /) must not silently disable
// the lane: it decodes to the same key.
func TestControlDecisionVerifierAcceptsStandardBase64PublicKey(t *testing.T) {
	var public ed25519.PublicKey
	var private ed25519.PrivateKey
	for {
		var err error
		public, private, err = ed25519.GenerateKey(rand.Reader)
		if err != nil {
			t.Fatal(err)
		}
		if strings.ContainsAny(base64.StdEncoding.EncodeToString(public), "+/") {
			break
		}
	}
	verifier, err := NewControlDecisionVerifier("key-1", base64.StdEncoding.EncodeToString(public))
	if err != nil {
		t.Fatalf("standard base64 key refused: %v", err)
	}
	if _, err := verifier.verify(signedControlFireToken(t, private, "key-1", validControlFireIntent())); err != nil {
		t.Fatalf("token refused under a standard-base64 key: %v", err)
	}
	if _, err := NewControlDecisionVerifier("key-1", "not a key"); err == nil {
		t.Fatal("garbage key accepted")
	}
}
