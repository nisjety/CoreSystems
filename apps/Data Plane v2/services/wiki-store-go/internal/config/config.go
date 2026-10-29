package config

import (
	"errors"
	"os"
	"strconv"
	"strings"
)

type Config struct {
	DatabaseURL                 string
	HTTPPort                    int
	GRPCPort                    int
	NatsURL                     string
	NatsToken                   string
	JWTAudience                 string
	JWTIssuer                   string
	JWKSURL                     string
	JWTPublicKeyFile            string
	EventSigningPrivateKeyPath  string
	AllowDisabledEventPublisher bool
	// GDPRSharedNats{URL,User,Password} configure a SECOND, narrowly-scoped
	// connection to the cross-plane control-shared-nats broker (identity
	// "wiki-store-gdpr"), used only by the GDPR org-erasure purge consumer
	// (internal/gdpr). Deliberately DISTINCT env var names from NatsURL's own
	// fallback chain above, which already treats the literal name
	// "SHARED_NATS_URL" as an alternate Data-Plane-LOCAL broker URL for the
	// wiki-event publisher — reusing any name that close (including the
	// commonly-used "NATS_SHARED_URL") here would invite exactly the kind of
	// operator mix-up this consumer must not repeat. Empty GDPRSharedNatsURL
	// disables the org-erasure consumer without affecting the plane-local
	// NATS client or the wiki-event publisher.
	GDPRSharedNatsURL      string
	GDPRSharedNatsUser     string
	GDPRSharedNatsPassword string
}

func Load() *Config {
	return &Config{
		DatabaseURL: envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		HTTPPort:    envIntOr("HTTP_PORT", 8011),
		// Model Plane gateway's DATAPLANE_WIKI_ADDR default is :50054.
		GRPCPort: envIntOr("GRPC_PORT", 50054),
		// §16.1.6 NATS env standardization — same fallback chain as Rust services.
		NatsURL: firstEnv(
			[]string{"DPV2_NATS_URL", "SHARED_NATS_URL", "NATS_URL", "NATS_LOCAL_URL"},
			"",
		),
		NatsToken:                  strings.TrimSpace(os.Getenv("DATAPLANE_NATS_TOKEN")),
		JWTAudience:                envOr("DATA_PLANE_AUTH_AUDIENCE", "data-plane"),
		JWTIssuer:                  envOr("AUTH_CORE_ISSUER", "http://localhost:3011/api/convex-auth"),
		JWKSURL:                    envOr("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		JWTPublicKeyFile:           envOr("JWT_PUBLIC_KEY_FILE", ""),
		EventSigningPrivateKeyPath: envOr("WIKI_EVENT_PRIVATE_KEY_PATH", ""),
		AllowDisabledEventPublisher: os.Getenv("ALLOW_UNVERIFIED_LEGACY_EVENTS") == "1" &&
			os.Getenv("ALLOW_INSECURE_DEV_DEFAULTS") == "1" && os.Getenv("ISOLATED_E2E") == "1",
		GDPRSharedNatsURL:      strings.TrimSpace(os.Getenv("WIKISTORE_GDPR_SHARED_NATS_URL")),
		GDPRSharedNatsUser:     strings.TrimSpace(os.Getenv("WIKISTORE_GDPR_SHARED_NATS_USER")),
		GDPRSharedNatsPassword: strings.TrimSpace(os.Getenv("WIKISTORE_GDPR_SHARED_NATS_PASSWORD")),
	}
}

// ValidateEventSecurity prevents a content-bearing NATS publisher from
// starting unless it has a producer-local signing key. An empty NATS URL
// explicitly disables asynchronous publication.
func (c *Config) ValidateEventSecurity() error {
	if strings.TrimSpace(c.NatsURL) == "" {
		if c.AllowDisabledEventPublisher {
			return nil
		}
		return errors.New("NATS URL and signed wiki event publisher are required in production posture")
	}
	if strings.TrimSpace(c.EventSigningPrivateKeyPath) == "" {
		return errors.New("WIKI_EVENT_PRIVATE_KEY_PATH is required when NATS is enabled")
	}
	return nil
}

// firstEnv returns the first non-empty value from `keys`, falling back to
// `fallback` if all are unset. Empty string disables the dependent feature.
func firstEnv(keys []string, fallback string) string {
	for _, k := range keys {
		if v := os.Getenv(k); v != "" {
			return v
		}
	}
	return fallback
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
