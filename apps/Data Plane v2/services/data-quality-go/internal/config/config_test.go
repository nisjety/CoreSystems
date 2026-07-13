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
