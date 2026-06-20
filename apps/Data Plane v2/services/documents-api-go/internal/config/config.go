package config

import (
	"fmt"
	"os"
	"strconv"
)

type Config struct {
	DatabaseURL    string
	NatsURL        string
	HTTPPort       int
	GRPCPort       int
	SharedNatsURL  string
	SharedNatsToken string
	InternalAPIKey string
	// UserCoreURL is the user-core base URL used to resolve a viewer's explicit
	// resource grants (the per-user authz facade). Per-user ownership filtering.
	UserCoreURL string
}

func Load() (*Config, error) {
	cfg := &Config{
		DatabaseURL:     envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		NatsURL:         envOr("NATS_URL", "nats://localhost:4232"),
		HTTPPort:        envIntOr("HTTP_PORT", 8010),
		GRPCPort:        envIntOr("GRPC_PORT", 50060),
		SharedNatsURL:   envOr("NATS_SHARED_URL", ""),
		SharedNatsToken: envOr("VELION_NATS_TOKEN", ""),
		InternalAPIKey:  envOr("INTERNAL_API_KEY", ""),
		UserCoreURL:     envOr("USER_CORE_URL", "http://user-core:8080"),
	}
	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL required")
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
