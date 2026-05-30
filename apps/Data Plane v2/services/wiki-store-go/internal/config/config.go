package config

import (
	"os"
	"strconv"
)

type Config struct {
	DatabaseURL string
	HTTPPort    int
	GRPCPort    int
	NatsURL     string
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
	}
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
