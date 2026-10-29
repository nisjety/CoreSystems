package clients

import "testing"

func TestNatsAuthOptionsPreferScopedUserPassword(t *testing.T) {
	t.Setenv("NATS_USER", "user-core-control")
	t.Setenv("NATS_PASSWORD", "0123456789abcdef0123456789abcdef")
	t.Setenv("NATS_TOKEN", "legacy-token-0123456789abcdef0123")
	t.Setenv("NATS_ALLOW_TOKEN_FALLBACK", "1")

	credential, err := selectNatsAuthCredential()
	if err != nil {
		t.Fatal(err)
	}
	if credential.User != "user-core-control" || credential.Password == "" || credential.Token != "" {
		t.Fatalf("credential = %+v", credential)
	}
}

func TestNatsAuthOptionsRejectImplicitTokenFallback(t *testing.T) {
	t.Setenv("NATS_USER", "")
	t.Setenv("NATS_PASSWORD", "")
	t.Setenv("NATS_TOKEN", "legacy-token-0123456789abcdef0123")
	t.Setenv("NATS_AUTH_TOKEN", "")
	t.Setenv("NATS_ALLOW_TOKEN_FALLBACK", "")

	if _, err := selectNatsAuthCredential(); err == nil {
		t.Fatal("implicit token fallback accepted")
	}
}

func TestNatsAuthOptionsRejectPartialOrWeakUserPassword(t *testing.T) {
	for _, test := range []struct {
		name     string
		user     string
		password string
	}{
		{name: "missing password", user: "user-core-control"},
		{name: "missing user", password: "0123456789abcdef0123456789abcdef"},
		{name: "short password", user: "user-core-control", password: "short"},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Setenv("NATS_USER", test.user)
			t.Setenv("NATS_PASSWORD", test.password)
			t.Setenv("NATS_TOKEN", "")
			t.Setenv("NATS_AUTH_TOKEN", "")
			if _, err := selectNatsAuthCredential(); err == nil {
				t.Fatal("unsafe NATS credential accepted")
			}
		})
	}
}
