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
}

func Load() *Config {
	return &Config{
		DatabaseURL:                 envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		NatsURL:                     envOr("NATS_URL", "nats://localhost:4232"),
		NatsToken:                   strings.TrimSpace(os.Getenv("DATAPLANE_NATS_TOKEN")),
		HTTPPort:                    envIntOr("HTTP_PORT", 8012),
		JWTAudience:                 envOr("DATA_PLANE_AUTH_AUDIENCE", "data-plane"),
		JWTIssuer:                   envOr("AUTH_CORE_ISSUER", "http://localhost:3011/api/convex-auth"),
		JWKSURL:                     envOr("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		JWTPublicKeyFile:            envOr("JWT_PUBLIC_KEY_FILE", ""),
		SignedCostEventsEnabled:     os.Getenv("SIGNED_COST_EVENTS_ENABLED") == "1",
		EmbeddingEventPublicKeyPath: strings.TrimSpace(os.Getenv("EMBEDDING_EVENT_PUBLIC_KEY_PATH")),
		RetrievalEventPublicKeyPath: strings.TrimSpace(os.Getenv("RETRIEVAL_EVENT_PUBLIC_KEY_PATH")),
	}
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
