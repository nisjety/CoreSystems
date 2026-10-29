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

func TestLoadReadsDedicatedGDPRSharedNATSCredentialsAndLeavesThemUnsetByDefault(t *testing.T) {
	cfg := Load()
	if cfg.GDPRSharedNatsURL != "" || cfg.GDPRSharedNatsUser != "" || cfg.GDPRSharedNatsPassword != "" {
		t.Fatalf("GDPR shared NATS credentials must be unset by default: %+v", cfg)
	}

	t.Setenv("WIKISTORE_GDPR_SHARED_NATS_URL", "nats://control-shared-nats:4222")
	t.Setenv("WIKISTORE_GDPR_SHARED_NATS_USER", "wiki-store-gdpr")
	t.Setenv("WIKISTORE_GDPR_SHARED_NATS_PASSWORD", "s3cret")

	cfg = Load()
	if cfg.GDPRSharedNatsURL != "nats://control-shared-nats:4222" {
		t.Fatalf("GDPRSharedNatsURL = %q", cfg.GDPRSharedNatsURL)
	}
	if cfg.GDPRSharedNatsUser != "wiki-store-gdpr" {
		t.Fatalf("GDPRSharedNatsUser = %q", cfg.GDPRSharedNatsUser)
	}
	if cfg.GDPRSharedNatsPassword != "s3cret" {
		t.Fatalf("GDPRSharedNatsPassword = %q", cfg.GDPRSharedNatsPassword)
	}

	// This consumer's dedicated env var names must never be satisfied by the
	// generic "NATS_SHARED_URL" name other services in this monorepo use —
	// that name is already claimed inside wiki-store-go's own NatsURL
	// fallback chain (as the confusingly-similar literal "SHARED_NATS_URL")
	// for a completely different, plane-local connection.
	t.Setenv("WIKISTORE_GDPR_SHARED_NATS_URL", "")
	t.Setenv("WIKISTORE_GDPR_SHARED_NATS_USER", "")
	t.Setenv("WIKISTORE_GDPR_SHARED_NATS_PASSWORD", "")
	t.Setenv("NATS_SHARED_URL", "nats://unrelated:4222")
	t.Setenv("SHARED_NATS_URL", "nats://plane-local:4222")

	cfg = Load()
	if cfg.GDPRSharedNatsURL != "" {
		t.Fatalf("GDPRSharedNatsURL must not fall back to a generic shared-NATS env var, got %q", cfg.GDPRSharedNatsURL)
	}
}
