package main

import "testing"

func TestAuthConfigFromEnvFailsClosed(t *testing.T) {
	for _, name := range []string{"CAPABILITY_CORE_AUTH_AUDIENCE", "AUTH_CORE_ISSUER", "AUTH_CORE_JWKS_URL"} {
		t.Run("missing "+name, func(t *testing.T) {
			t.Setenv("CAPABILITY_CORE_AUTH_AUDIENCE", "capability-core")
			t.Setenv("AUTH_CORE_ISSUER", "https://auth.example.test")
			t.Setenv("AUTH_CORE_JWKS_URL", "https://auth.example.test/jwks")
			t.Setenv(name, "")
			if _, err := authConfigFromEnv(); err == nil {
				t.Fatalf("missing %s did not fail closed", name)
			}
		})
	}
}

func TestAuthConfigFromEnvReturnsExplicitTrustBoundary(t *testing.T) {
	t.Setenv("CAPABILITY_CORE_AUTH_AUDIENCE", "capability-core")
	t.Setenv("AUTH_CORE_ISSUER", "https://auth.example.test")
	t.Setenv("AUTH_CORE_JWKS_URL", "https://auth.example.test/jwks")

	config, err := authConfigFromEnv()
	if err != nil {
		t.Fatalf("authConfigFromEnv(): %v", err)
	}
	if len(config.Audiences) != 1 || config.Audiences[0] != "capability-core" {
		t.Fatalf("unexpected audiences: %v", config.Audiences)
	}
	if config.Issuer != "https://auth.example.test" || config.JWKSURL != "https://auth.example.test/jwks" {
		t.Fatalf("unexpected config: %+v", config)
	}
}
