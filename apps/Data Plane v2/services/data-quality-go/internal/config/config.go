package config

import (
	"os"
	"strconv"
)

type Config struct {
	DatabaseURL      string
	HTTPPort         int
	JWTAudience      string
	JWTIssuer        string
	JWKSURL          string
	JWTPublicKeyFile string
}

func Load() *Config {
	return &Config{
		DatabaseURL:      envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		HTTPPort:         envIntOr("HTTP_PORT", 8013),
		JWTAudience:      envOr("DATA_PLANE_AUTH_AUDIENCE", "data-plane"),
		JWTIssuer:        envOr("AUTH_CORE_ISSUER", "http://localhost:3011/api/convex-auth"),
		JWKSURL:          envOr("AUTH_CORE_JWKS_URL", "http://auth-core:3011/api/convex-auth/jwks"),
		JWTPublicKeyFile: envOr("JWT_PUBLIC_KEY_FILE", ""),
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
