package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

type Config struct {
	HTTPPort       int
	ServiceName    string
	InternalAPIKey string
	DatabaseURL    string
}

func Load() (*Config, error) {
	cfg := &Config{
		HTTPPort:       getEnvInt("PORT", 3164),
		ServiceName:    getEnv("SERVICE_NAME", "leads-core"),
		InternalAPIKey: strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		DatabaseURL:    strings.TrimSpace(os.Getenv("DATABASE_URL")),
	}
	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	return cfg, nil
}

func getEnv(key, fallback string) string {
	if v := strings.TrimSpace(os.Getenv(key)); v != "" {
		return v
	}
	return fallback
}

func getEnvInt(key string, fallback int) int {
	v := strings.TrimSpace(os.Getenv(key))
	if v == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(v)
	if err != nil || parsed < 1 || parsed > 65535 {
		return fallback
	}
	return parsed
}
