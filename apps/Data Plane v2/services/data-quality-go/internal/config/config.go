package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

type Config struct {
	DatabaseURL      string
	HTTPPort         int
	JWTAudience      string
	JWTIssuer        string
	JWKSURL          string
	JWTPublicKeyFile string

	// SharedNatsURL/SharedNatsUser/SharedNatsPassword configure a dedicated
	// connection to the shared cross-plane broker (control-shared-nats), used
	// ONLY by the GDPR org-erasure consumer (internal/gdpr). data-quality-go
	// has no plane-local NATS client of its own (see cmd/main.go) — this is
	// deliberately its own narrowly-scoped identity, never reused from
	// another service's credentials, per apps/Control Plane/audit-core/
	// internal/provisioner/provisioner.go's one-identity-per-service rule.
	// Named NATS_SHARED_URL/_USER/_PASSWORD to match the convention already
	// used by documents-api-go and other Data Plane v2 services for their own
	// (distinct) shared-broker identities.
	SharedNatsURL      string
	SharedNatsUser     string
	SharedNatsPassword string
	// GDPRConsumerRequired fails startup when true and the shared-broker
	// credentials/durable consumer are missing or unbindable, keeping org
	// erasure fail-closed in production. Defaults to permissive (false) so a
	// local/dev boot without the shared broker configured still starts.
	GDPRConsumerRequired bool
}

func Load() (*Config, error) {
	cfg := &Config{
		DatabaseURL:      envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		HTTPPort:         envIntOr("HTTP_PORT", 8013),
		JWTAudience:      envOr("DATA_PLANE_AUTH_AUDIENCE", "data-plane"),
		JWTIssuer:        envOr("AUTH_CORE_ISSUER", "http://localhost:3011/api/convex-auth"),
		JWKSURL:          envOr("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		JWTPublicKeyFile: envOr("JWT_PUBLIC_KEY_FILE", ""),

		SharedNatsURL:        strings.TrimSpace(os.Getenv("NATS_SHARED_URL")),
		SharedNatsUser:       strings.TrimSpace(os.Getenv("NATS_SHARED_USER")),
		SharedNatsPassword:   strings.TrimSpace(os.Getenv("NATS_SHARED_PASSWORD")),
		GDPRConsumerRequired: os.Getenv("GDPR_DURABLE_CONSUMER_REQUIRED") == "1",
	}

	sharedConfigured := cfg.SharedNatsURL != "" || cfg.SharedNatsUser != "" || cfg.SharedNatsPassword != ""
	if sharedConfigured && (cfg.SharedNatsURL == "" || cfg.SharedNatsUser == "" || cfg.SharedNatsPassword == "") {
		return nil, fmt.Errorf("NATS_SHARED_URL, NATS_SHARED_USER, and NATS_SHARED_PASSWORD must be configured together")
	}
	if cfg.SharedNatsPassword != "" && len(cfg.SharedNatsPassword) < 32 {
		return nil, fmt.Errorf("NATS_SHARED_PASSWORD must contain at least 32 characters")
	}
	if cfg.GDPRConsumerRequired && !sharedConfigured {
		return nil, fmt.Errorf("scoped shared NATS credentials are required for the durable GDPR org-erasure consumer")
	}

	return cfg, nil
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

func envIntOr(key string, fallback int) int {
	if v := os.Getenv(key); v != "" {
		if n, err := strconv.Atoi(v); err == nil {
			return n
		}
	}
	return fallback
}
