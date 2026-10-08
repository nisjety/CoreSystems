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

func TestLoadDefaultsForTheContextualReleaseGates(t *testing.T) {
	clearSharedNatsEnv(t)
	t.Setenv("CONTEXT_COVERAGE_THRESHOLD_PERCENT", "")
	t.Setenv("SPARSE_INDEX_PARITY_URL", "")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}
	if cfg.CoverageThresholdPercent != 99 {
		t.Fatalf("coverage threshold = %v, want the design's 99", cfg.CoverageThresholdPercent)
	}
	if cfg.SparseIndexParityURL != "" {
		t.Fatalf("parity URL = %q, want empty: the gate then reports unavailable, never a pass", cfg.SparseIndexParityURL)
	}
}

func TestLoadAcceptsAConfiguredThresholdAndParityEndpoint(t *testing.T) {
	clearSharedNatsEnv(t)
	t.Setenv("CONTEXT_COVERAGE_THRESHOLD_PERCENT", "97.5")
	t.Setenv("SPARSE_INDEX_PARITY_URL", "http://quickwit-adapter:8008/admin/index-parity")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}
	if cfg.CoverageThresholdPercent != 97.5 || cfg.SparseIndexParityURL != "http://quickwit-adapter:8008/admin/index-parity" {
		t.Fatalf("unexpected gate config: %+v", cfg)
	}
}

// A threshold that cannot mean "a percentage of units" must stop the service at
// boot, not quietly make the coverage gate always pass (0, negative, above 100) or
// never pass, or read as the default (a typo).
func TestLoadRejectsAnInvalidCoverageThreshold(t *testing.T) {
	for _, value := range []string{"0", "-1", "100.5", "101", "ninety-nine", "NaN", "Inf", "99%"} {
		t.Run(value, func(t *testing.T) {
			clearSharedNatsEnv(t)
			t.Setenv("SPARSE_INDEX_PARITY_URL", "")
			t.Setenv("CONTEXT_COVERAGE_THRESHOLD_PERCENT", value)
			if _, err := Load(); err == nil {
				t.Fatalf("CONTEXT_COVERAGE_THRESHOLD_PERCENT=%q was accepted", value)
			}
		})
	}
}

// The parity URL receives the caller's bearer: it must be an absolute http(s) URL
// the operator wrote, with no credentials of its own.
func TestLoadRejectsAnInvalidParityEndpoint(t *testing.T) {
	for _, value := range []string{
		"quickwit-adapter:8008/admin/index-parity",
		"/admin/index-parity",
		"ftp://quickwit-adapter/admin/index-parity",
		"http://user:secret@quickwit-adapter:8008/admin/index-parity",
		"http://quickwit-adapter:8008/admin/index-parity#fragment",
		"http:///admin/index-parity",
	} {
		t.Run(value, func(t *testing.T) {
			clearSharedNatsEnv(t)
			t.Setenv("CONTEXT_COVERAGE_THRESHOLD_PERCENT", "")
			t.Setenv("SPARSE_INDEX_PARITY_URL", value)
			if _, err := Load(); err == nil {
				t.Fatalf("SPARSE_INDEX_PARITY_URL=%q was accepted", value)
			}
		})
	}
}

func clearSharedNatsEnv(t *testing.T) {
	t.Helper()
	t.Setenv("NATS_SHARED_URL", "")
	t.Setenv("NATS_SHARED_USER", "")
	t.Setenv("NATS_SHARED_PASSWORD", "")
	t.Setenv("GDPR_DURABLE_CONSUMER_REQUIRED", "")
}
