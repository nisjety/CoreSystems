package config

import (
	"os"
	"strconv"
)

type Config struct {
	DatabaseURL string
	HTTPPort    int
}

func Load() *Config {
	return &Config{
		DatabaseURL: envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		HTTPPort:    envIntOr("HTTP_PORT", 8013),
	}
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
