package config

import (
	"errors"
	"os"
	"strconv"
	"strings"
)

type Config struct {
	DatabaseURL                 string
	NatsURL                     string
	NatsToken                   string
	HTTPPort                    int
	JWTAudience                 string
	JWTIssuer                   string
	JWKSURL                     string
	JWTPublicKeyFile            string
	SignedCostEventsEnabled     bool
	EmbeddingEventPublicKeyPath string
	RetrievalEventPublicKeyPath string
	// SharedNats* configure this service's OWN dedicated identity on the
	// cross-plane shared broker (control-shared-nats), used ONLY by the
	// GDPR org-erasure durable consumer (internal/gdpr). This is a
	// deliberately separate connection from NatsURL/NatsToken above, which
	// is the plane-local Data Plane v2 broker the cost-ledger consumer and
	// job executor already use — never reuse that connection or any other
	// service's shared identity for this consumer.
	SharedNatsURL      string
	SharedNatsUser     string
	SharedNatsPassword string
	// GDPROrgPurgeConsumerRequired gates the "data-orchestrator-org-erasure"
	// durable (internal/gdpr): when true, the service fails to boot if the
	// pre-provisioned consumer can't be bound.
	GDPROrgPurgeConsumerRequired bool
}

func Load() *Config {
	return &Config{
		DatabaseURL:                  envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		NatsURL:                      envOr("NATS_URL", "nats://localhost:4232"),
		NatsToken:                    strings.TrimSpace(os.Getenv("DATAPLANE_NATS_TOKEN")),
		HTTPPort:                     envIntOr("HTTP_PORT", 8012),
		JWTAudience:                  envOr("DATA_PLANE_AUTH_AUDIENCE", "data-plane"),
		JWTIssuer:                    envOr("AUTH_CORE_ISSUER", "http://localhost:3011/api/convex-auth"),
		JWKSURL:                      envOr("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		JWTPublicKeyFile:             envOr("JWT_PUBLIC_KEY_FILE", ""),
		SignedCostEventsEnabled:      os.Getenv("SIGNED_COST_EVENTS_ENABLED") == "1",
		EmbeddingEventPublicKeyPath:  strings.TrimSpace(os.Getenv("EMBEDDING_EVENT_PUBLIC_KEY_PATH")),
		RetrievalEventPublicKeyPath:  strings.TrimSpace(os.Getenv("RETRIEVAL_EVENT_PUBLIC_KEY_PATH")),
		SharedNatsURL:                strings.TrimSpace(os.Getenv("NATS_SHARED_URL")),
		SharedNatsUser:               strings.TrimSpace(os.Getenv("NATS_SHARED_USER")),
		SharedNatsPassword:           strings.TrimSpace(os.Getenv("NATS_SHARED_PASSWORD")),
		GDPROrgPurgeConsumerRequired: os.Getenv("GDPR_ORG_PURGE_CONSUMER_REQUIRED") == "1",
	}
}

// ValidateGDPRConsumer enforces the shared-broker credential contract for the
// GDPR org-erasure durable consumer (internal/gdpr): all three
// NATS_SHARED_* variables must be configured together (never partially), the
// password must meet a minimum strength bar, and if the consumer is marked
// required, the credentials must actually be present so the service fails
// closed instead of silently running without erasure coverage.
func (c *Config) ValidateGDPRConsumer() error {
	sharedConfigured := c.SharedNatsURL != "" || c.SharedNatsUser != "" || c.SharedNatsPassword != ""
	if sharedConfigured && (c.SharedNatsURL == "" || c.SharedNatsUser == "" || c.SharedNatsPassword == "") {
		return errors.New("NATS_SHARED_URL, NATS_SHARED_USER, and NATS_SHARED_PASSWORD must be configured together")
	}
	if c.SharedNatsPassword != "" && len(c.SharedNatsPassword) < 32 {
		return errors.New("NATS_SHARED_PASSWORD must contain at least 32 characters")
	}
	if c.GDPROrgPurgeConsumerRequired && !sharedConfigured {
		return errors.New("scoped shared NATS credentials are required for the durable GDPR org-purge consumer")
	}
	return nil
}

func (c *Config) ValidateSignedCostEvents() error {
	if !c.SignedCostEventsEnabled {
		return nil
	}
	if c.EmbeddingEventPublicKeyPath == "" || c.RetrievalEventPublicKeyPath == "" {
		return errors.New("signed cost events require embedding and retrieval public keys")
	}
	return nil
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
