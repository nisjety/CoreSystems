package config

import "testing"

func TestEventPublisherRequiresPrivateKeyWhenNATSIsConfigured(t *testing.T) {
	cfg := &Config{NatsURL: "nats://nats:4222"}
	if err := cfg.ValidateEventSecurity(); err == nil {
		t.Fatal("production publisher accepted NATS without a signing key")
	}

	cfg.EventSigningPrivateKeyPath = "/run/event-keys/wiki-events.pem"
	if err := cfg.ValidateEventSecurity(); err != nil {
		t.Fatalf("valid signed publisher configuration rejected: %v", err)
	}

	cfg.NatsURL = ""
	cfg.EventSigningPrivateKeyPath = ""
	if err := cfg.ValidateEventSecurity(); err == nil {
		t.Fatal("production posture accepted a disabled unsigned event publisher")
	}
	cfg.AllowDisabledEventPublisher = true
	if err := cfg.ValidateEventSecurity(); err != nil {
		t.Fatalf("explicit isolated fixture should allow disabled publisher: %v", err)
	}
}

func TestLoadIncludesFailClosedAuthDefaults(t *testing.T) {
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "")
	t.Setenv("AUTH_CORE_ISSUER", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")

	cfg := Load()

	if cfg.JWTAudience != "data-plane" {
		t.Fatalf("JWT audience = %q", cfg.JWTAudience)
	}
	if cfg.JWTIssuer == "" || cfg.JWKSURL == "" {
		t.Fatalf("auth defaults must be complete: issuer=%q jwks=%q", cfg.JWTIssuer, cfg.JWKSURL)
	}
}

func TestLoadAllowsExplicitStaticKeyConfiguration(t *testing.T) {
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "wiki-service")
	t.Setenv("AUTH_CORE_ISSUER", "https://auth.example/issuer")
	t.Setenv("AUTH_CORE_JWKS_URL", "https://auth.example/jwks")
	t.Setenv("JWT_PUBLIC_KEY_FILE", "/run/secrets/wiki-public.pem")

	cfg := Load()

	if cfg.JWTAudience != "wiki-service" || cfg.JWTIssuer != "https://auth.example/issuer" {
		t.Fatalf("unexpected claims contract: %+v", cfg)
	}
	if cfg.JWKSURL != "https://auth.example/jwks" || cfg.JWTPublicKeyFile != "/run/secrets/wiki-public.pem" {
		t.Fatalf("unexpected verifier config: %+v", cfg)
	}
}

func TestLoadReadsDedicatedDataPlaneNATSToken(t *testing.T) {
	t.Setenv("DATAPLANE_NATS_TOKEN", "opaque-token")

	cfg := Load()
	if cfg.NatsToken != "opaque-token" {
		t.Fatalf("NatsToken was not loaded from the dedicated environment variable")
	}
}
