package config

import "testing"

func TestLoadUsesFailClosedControlTokenDefaults(t *testing.T) {
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "")
	t.Setenv("AUTH_CORE_ISSUER", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")

	cfg := Load()

	if cfg.JWTAudience != "data-plane" {
		t.Fatalf("JWT audience = %q", cfg.JWTAudience)
	}
	if cfg.JWTIssuer != "http://localhost:3011/api/convex-auth" {
		t.Fatalf("JWT issuer = %q", cfg.JWTIssuer)
	}
	if cfg.JWKSURL != "http://auth-core:3011/api/convex-auth/jwks" {
		t.Fatalf("JWKS URL = %q", cfg.JWKSURL)
	}
}

func TestLoadAcceptsExplicitVerificationConfiguration(t *testing.T) {
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "custom-data-plane")
	t.Setenv("AUTH_CORE_ISSUER", "https://auth.example/issuer")
	t.Setenv("AUTH_CORE_JWKS_URL", "https://auth.example/jwks")
	t.Setenv("JWT_PUBLIC_KEY_FILE", "/run/secrets/data-plane.pub")

	cfg := Load()

	if cfg.JWTAudience != "custom-data-plane" || cfg.JWTIssuer != "https://auth.example/issuer" {
		t.Fatalf("unexpected identity config: %+v", cfg)
	}
	if cfg.JWKSURL != "https://auth.example/jwks" || cfg.JWTPublicKeyFile != "/run/secrets/data-plane.pub" {
		t.Fatalf("unexpected key config: %+v", cfg)
	}
}

func TestLoadReadsDedicatedDataPlaneNATSToken(t *testing.T) {
	t.Setenv("DATAPLANE_NATS_TOKEN", "opaque-token")

	cfg := Load()
	if cfg.NatsToken != "opaque-token" {
		t.Fatalf("NatsToken was not loaded from the dedicated environment variable")
	}
}

func TestSignedCostConsumerConfigurationDefaultsOff(t *testing.T) {
	t.Setenv("SIGNED_COST_EVENTS_ENABLED", "")
	t.Setenv("EMBEDDING_EVENT_PUBLIC_KEY_PATH", "")
	t.Setenv("RETRIEVAL_EVENT_PUBLIC_KEY_PATH", "")

	cfg := Load()
	if cfg.SignedCostEventsEnabled {
		t.Fatal("signed cost consumer unexpectedly enabled")
	}
}

func TestSignedCostConsumerConfigurationRequiresBothProducerKeys(t *testing.T) {
	t.Setenv("SIGNED_COST_EVENTS_ENABLED", "1")
	t.Setenv("EMBEDDING_EVENT_PUBLIC_KEY_PATH", "/run/event-keys/embedding.pub")
	t.Setenv("RETRIEVAL_EVENT_PUBLIC_KEY_PATH", "")

	cfg := Load()
	if err := cfg.ValidateSignedCostEvents(); err == nil {
		t.Fatal("signed consumer accepted incomplete producer registry")
	}

	t.Setenv("RETRIEVAL_EVENT_PUBLIC_KEY_PATH", "/run/event-keys/retrieval.pub")
	cfg = Load()
	if err := cfg.ValidateSignedCostEvents(); err != nil {
		t.Fatalf("complete signed consumer configuration failed: %v", err)
	}
}
