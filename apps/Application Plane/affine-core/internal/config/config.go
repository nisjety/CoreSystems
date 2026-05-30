package config

import (
	"fmt"
	"os"
	"strconv"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort            int
	DatabaseURL         string
	NATSURL             string
	NATSToken           string
	ServiceName         string
	InternalAPIKey      string
	AuthServiceURL      string
	UserServiceURL      string
	AffineRuntimeURL    string
	AffineAdminEmail    string
	AffineAdminPassword string
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:    getEnvInt("PORT", 3180),
		DatabaseURL: getEnv("DATABASE_URL", ""),
		// AFFiNE consumes cross-plane events from the Velion frontend broker.
		// Prefer VELION_NATS_* and keep the older shared names plus NATS_URL /
		// NATS_TOKEN as compatibility fallbacks while other planes migrate.
		NATSURL:             getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", getEnv("NATS_URL", "nats://velion-nats:4222"))),
		NATSToken:           getEnv("VELION_NATS_TOKEN", getEnv("NATS_SHARED_TOKEN", getEnv("NATS_TOKEN", ""))),
		ServiceName:         getEnv("SERVICE_NAME", "affine-core"),
		InternalAPIKey:      getEnv("INTERNAL_API_KEY", ""),
		AuthServiceURL:      getEnv("AUTH_SERVICE_URL", "http://auth-core:3011"),
		UserServiceURL:      getEnv("USER_SERVICE_URL", "http://user-core:3012"),
		AffineRuntimeURL:    getEnv("AFFINE_RUNTIME_URL", "http://affine-runtime:3010"),
		AffineAdminEmail:    getEnv("AFFINE_ADMIN_EMAIL", "admin@localhost"),
		AffineAdminPassword: getEnv("AFFINE_ADMIN_PASSWORD", ""),
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
	}

	return cfg, nil
}

func getEnv(key, fallback string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return fallback
}

func getEnvInt(key string, fallback int) int {
	value := os.Getenv(key)
	if value == "" {
		return fallback
	}

	parsed, err := strconv.Atoi(value)
	if err != nil {
		return fallback
	}
	return parsed
}
