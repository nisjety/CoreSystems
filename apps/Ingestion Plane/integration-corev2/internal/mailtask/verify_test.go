package mailtask

import (
	"encoding/base64"
	"testing"
)

func basicAuthHeader(user, pass string) string {
	return "Basic " + base64.StdEncoding.EncodeToString([]byte(user+":"+pass))
}

func TestVerifyWebhookAuthAcceptsTheConfiguredSecret(t *testing.T) {
	if !VerifyWebhookAuth(basicAuthHeader("postmark", "the-secret"), "the-secret") {
		t.Fatal("expected matching Basic credentials to verify")
	}
}

func TestVerifyWebhookAuthRejectsWrongSecret(t *testing.T) {
	if VerifyWebhookAuth(basicAuthHeader("postmark", "wrong"), "the-secret") {
		t.Fatal("expected mismatched secret to fail")
	}
}

func TestVerifyWebhookAuthFailsClosedWhenSecretIsUnconfigured(t *testing.T) {
	if VerifyWebhookAuth(basicAuthHeader("postmark", "anything"), "") {
		t.Fatal("empty expectedSecret must always fail (off by default)")
	}
}

func TestVerifyWebhookAuthRejectsMissingOrMalformedHeader(t *testing.T) {
	for _, header := range []string{"", "Bearer sometoken", "Basic not-valid-base64!!", "Basic " + base64.StdEncoding.EncodeToString([]byte("no-colon"))} {
		if VerifyWebhookAuth(header, "the-secret") {
			t.Fatalf("header %q should not verify", header)
		}
	}
}

func TestVerifySenderRequiresBothAuthResultPassAndMatchingVerifiedEmail(t *testing.T) {
	tests := []struct {
		name          string
		authResult    string
		fromEmail     string
		verifiedEmail string
		want          bool
	}{
		{name: "pass and match", authResult: "pass", fromEmail: "owner@example.com", verifiedEmail: "owner@example.com", want: true},
		{name: "pass and match, case-insensitive", authResult: "pass", fromEmail: "Owner@Example.com", verifiedEmail: "owner@example.com", want: true},
		{name: "auth result fail", authResult: "fail", fromEmail: "owner@example.com", verifiedEmail: "owner@example.com", want: false},
		{name: "auth result none", authResult: "none", fromEmail: "owner@example.com", verifiedEmail: "owner@example.com", want: false},
		{name: "pass but different sender", authResult: "pass", fromEmail: "attacker@example.com", verifiedEmail: "owner@example.com", want: false},
		{name: "pass but no verified email on file", authResult: "pass", fromEmail: "owner@example.com", verifiedEmail: "", want: false},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			event := InboundMailTaskEvent{AuthResult: test.authResult, FromEmail: test.fromEmail}
			if got := VerifySender(event, test.verifiedEmail); got != test.want {
				t.Fatalf("VerifySender() = %v, want %v", got, test.want)
			}
		})
	}
}

func TestVerifyReceivingDomainFailsClosedWhenUnconfigured(t *testing.T) {
	if VerifyReceivingDomain("tasks.verevon.ai", "") {
		t.Fatal("empty expectedDomain must always fail (off by default)")
	}
}

func TestVerifyReceivingDomainIsCaseInsensitive(t *testing.T) {
	if !VerifyReceivingDomain("Tasks.Verevon.AI", "tasks.verevon.ai") {
		t.Fatal("expected a case-insensitive domain match")
	}
}

func TestVerifyReceivingDomainRejectsAnUnrelatedDomain(t *testing.T) {
	if VerifyReceivingDomain("attacker.example.com", "tasks.verevon.ai") {
		t.Fatal("expected an unrelated domain to be rejected")
	}
}
