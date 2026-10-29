package nats

import "testing"

func TestSelectRuntimeCredentialPrefersScopedPrincipalAndGatesToken(t *testing.T) {
	got, err := selectRuntimeCredential("application-notification", "0123456789abcdef0123456789abcdef")
	if err != nil || got.User != "application-notification" {
		t.Fatalf("scoped = %+v, %v", got, err)
	}
	if _, err := selectRuntimeCredential("", "abcdef0123456789abcdef0123456789"); err == nil {
		t.Fatal("missing scoped user was accepted")
	}
}
