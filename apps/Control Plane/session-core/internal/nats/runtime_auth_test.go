package nats

import "testing"

func TestSelectRuntimeCredentialPrefersScopedPrincipal(t *testing.T) {
	got, err := selectRuntimeCredential(Credentials{
		User:     "session-core-control",
		Password: "0123456789abcdef0123456789abcdef",
		Token:    "abcdef0123456789abcdef0123456789",
	})
	if err != nil {
		t.Fatal(err)
	}
	if got.User != "session-core-control" || got.Password == "" || got.Token != "" {
		t.Fatalf("credential = %+v", got)
	}
}

func TestSelectRuntimeCredentialRequiresExplicitTokenFallback(t *testing.T) {
	credentials := Credentials{Token: "abcdef0123456789abcdef0123456789"}
	if _, err := selectRuntimeCredential(credentials); err == nil {
		t.Fatal("implicit shared-token fallback accepted")
	}
	credentials.AllowTokenFallback = true
	got, err := selectRuntimeCredential(credentials)
	if err != nil || got.Token == "" {
		t.Fatalf("explicit fallback = %+v, %v", got, err)
	}
}

func TestSelectRuntimeCredentialRejectsPartialPair(t *testing.T) {
	if _, err := selectRuntimeCredential(Credentials{User: "session-core-control"}); err == nil {
		t.Fatal("partial scoped credential accepted")
	}
}

func TestSharedCredentialDoesNotInheritLocalEnvironment(t *testing.T) {
	t.Setenv("NATS_USER", "session-core-control")
	t.Setenv("NATS_PASSWORD", "0123456789abcdef0123456789abcdef")
	got, err := selectRuntimeCredential(Credentials{
		Token:              "abcdef0123456789abcdef0123456789",
		AllowTokenFallback: true,
	})
	if err != nil {
		t.Fatal(err)
	}
	if got.Token == "" || got.User != "" || got.Password != "" {
		t.Fatalf("shared credential leaked local environment: %+v", got)
	}
}
