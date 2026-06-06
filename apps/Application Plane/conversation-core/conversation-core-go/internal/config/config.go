package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort       int
	DatabaseURL    string
	NATSURL        string
	NATSToken      string
	ServiceName    string
	InternalAPIKey string
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:       getEnvInt("PORT", 3160),
		DatabaseURL:    strings.TrimSpace(getEnv("DATABASE_URL", "")),
		NATSURL:        strings.TrimSpace(getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", getEnv("NATS_URL", "nats://nats:4222")))),
		NATSToken:      strings.TrimSpace(getEnv("VELION_NATS_TOKEN", getEnv("NATS_SHARED_TOKEN", getEnv("NATS_TOKEN", "")))),
		ServiceName:    strings.TrimSpace(getEnv("SERVICE_NAME", "conversation-core-go")),
		InternalAPIKey: strings.TrimSpace(getEnv("INTERNAL_API_KEY", "")),
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
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	return value
}

func getEnvInt(key string, fallback int) int {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(value)
	if err != nil || parsed < 1 || parsed > 65535 {
		return fallback
	}
	return parsed
}
