package nats

import "testing"

func TestSelectRuntimeCredentialPrefersScopedPrincipal(t *testing.T) {
	t.Setenv("NATS_USER", "control-runtime")
	t.Setenv("NATS_PASSWORD", "0123456789abcdef0123456789abcdef")
	t.Setenv("NATS_ALLOW_TOKEN_FALLBACK", "1")
	got, err := selectRuntimeCredential("abcdef0123456789abcdef0123456789")
	if err != nil {
		t.Fatal(err)
	}
	if got.User != "control-runtime" || got.Password == "" || got.Token != "" {
		t.Fatalf("credential = %+v", got)
	}
}

func TestSelectRuntimeCredentialRequiresExplicitTokenFallback(t *testing.T) {
	t.Setenv("NATS_USER", "")
	t.Setenv("NATS_PASSWORD", "")
	t.Setenv("NATS_ALLOW_TOKEN_FALLBACK", "")
	if _, err := selectRuntimeCredential("abcdef0123456789abcdef0123456789"); err == nil {
		t.Fatal("implicit shared-token fallback accepted")
	}
	t.Setenv("NATS_ALLOW_TOKEN_FALLBACK", "1")
	got, err := selectRuntimeCredential("abcdef0123456789abcdef0123456789")
	if err != nil || got.Token == "" {
		t.Fatalf("explicit fallback = %+v, %v", got, err)
	}
}

func TestSelectRuntimeCredentialRejectsPartialPair(t *testing.T) {
	t.Setenv("NATS_USER", "control-runtime")
	t.Setenv("NATS_PASSWORD", "")
	if _, err := selectRuntimeCredential(""); err == nil {
		t.Fatal("partial scoped credential accepted")
	}
}
