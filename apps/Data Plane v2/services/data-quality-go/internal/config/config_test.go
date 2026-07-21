package config

import (
	"strings"
	"testing"
)

func TestLoadUsesFailClosedControlTokenDefaults(t *testing.T) {
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "")
	t.Setenv("AUTH_CORE_ISSUER", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")
	clearSharedNatsEnv(t)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}

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
	clearSharedNatsEnv(t)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}

	if cfg.JWTAudience != "custom-data-plane" || cfg.JWTIssuer != "https://auth.example/issuer" {
		t.Fatalf("unexpected identity config: %+v", cfg)
	}
	if cfg.JWKSURL != "https://auth.example/jwks" || cfg.JWTPublicKeyFile != "/run/secrets/data-plane.pub" {
		t.Fatalf("unexpected key config: %+v", cfg)
	}
}

func TestLoadAcceptsFullyConfiguredSharedNats(t *testing.T) {
	clearSharedNatsEnv(t)
	t.Setenv("NATS_SHARED_URL", "nats://control-shared-nats:4222")
	t.Setenv("NATS_SHARED_USER", "data-quality-gdpr")
	t.Setenv("NATS_SHARED_PASSWORD", strings.Repeat("x", 32))

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}
	if cfg.SharedNatsURL != "nats://control-shared-nats:4222" || cfg.SharedNatsUser != "data-quality-gdpr" {
		t.Fatalf("unexpected shared NATS config: %+v", cfg)
	}
}

func TestLoadRejectsPartiallyConfiguredSharedNats(t *testing.T) {
	clearSharedNatsEnv(t)
	t.Setenv("NATS_SHARED_URL", "nats://control-shared-nats:4222")
	// NATS_SHARED_USER / NATS_SHARED_PASSWORD deliberately left unset.

	if _, err := Load(); err == nil {
		t.Fatal("expected an error for partially-configured shared NATS credentials")
	}
}

func TestLoadRejectsShortSharedNatsPassword(t *testing.T) {
	clearSharedNatsEnv(t)
	t.Setenv("NATS_SHARED_URL", "nats://control-shared-nats:4222")
	t.Setenv("NATS_SHARED_USER", "data-quality-gdpr")
	t.Setenv("NATS_SHARED_PASSWORD", "too-short")

	if _, err := Load(); err == nil {
		t.Fatal("expected an error for an under-length shared NATS password")
	}
}

func TestLoadRejectsRequiredGdprConsumerWithoutSharedNats(t *testing.T) {
	clearSharedNatsEnv(t)
	t.Setenv("GDPR_DURABLE_CONSUMER_REQUIRED", "1")

	if _, err := Load(); err == nil {
		t.Fatal("expected an error when the GDPR consumer is required but shared NATS is unconfigured")
	}
}

func clearSharedNatsEnv(t *testing.T) {
	t.Helper()
	t.Setenv("NATS_SHARED_URL", "")
	t.Setenv("NATS_SHARED_USER", "")
	t.Setenv("NATS_SHARED_PASSWORD", "")
	t.Setenv("GDPR_DURABLE_CONSUMER_REQUIRED", "")
}
