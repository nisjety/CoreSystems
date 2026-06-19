package config

import (
	"fmt"
	"net/url"
	"os"
	"strings"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort        int
	GRPCPort        int
	MetricsPort     int
	DatabaseURL     string
	NATSURL         string
	NATSToken       string
	NATSSharedURL   string
	NATSSharedToken string
	AuthServiceURL  string
	UserServiceURL  string
	ServiceName     string
	Redis           RedisConfig
}

type RedisConfig struct {
	Host     string
	Port     string
	Password string
	DB       int
	Enabled  bool
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:        getEnvInt("HTTP_PORT", 3013),
		GRPCPort:        getEnvInt("GRPC_PORT", 9090),
		MetricsPort:     getEnvInt("METRICS_PORT", 9091),
		DatabaseURL:     getEnv("DATABASE_URL", ""),
		NATSURL:         getEnv("NATS_URL", "nats://controlplane-nats:4222"),
		NATSToken:       getEnv("NATS_TOKEN", getEnv("NATS_AUTH_TOKEN", "")),
		NATSSharedURL:   getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", "")),
		NATSSharedToken: getEnv("VELION_NATS_TOKEN", getEnv("NATS_SHARED_TOKEN", "")),
		AuthServiceURL:  getEnv("AUTH_SERVICE_URL", "http://auth-service:3011"),
		UserServiceURL:  getEnv("USER_SERVICE_URL", "http://user-service:3012"),
		ServiceName:     getEnv("SERVICE_NAME", "org-core"),
		Redis: RedisConfig{
			Host:     getEnv("DRAGONFLY_HOST", getEnv("CACHE_HOST", getEnv("REDIS_HOST", "controlplane-dragonfly"))),
			Port:     getEnv("DRAGONFLY_PORT", getEnv("CACHE_PORT", getEnv("REDIS_PORT", "6379"))),
			Password: getEnv("DRAGONFLY_PASSWORD", getEnv("CACHE_PASSWORD", getEnv("REDIS_PASSWORD", ""))),
			DB:       getEnvInt("DRAGONFLY_DB", getEnvInt("CACHE_DB", getEnvInt("REDIS_DB", 3))),
			Enabled:  getEnvBool("DRAGONFLY_ENABLED", getEnvBool("CACHE_ENABLED", getEnvBool("REDIS_ENABLED", false))),
		},
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}

	// Transit hardening: enforce TLS to managed Postgres. If the DSN does not
	// already carry an sslmode, apply one. The mode is taken from DB_SSLMODE
	// when set; otherwise it defaults to "require" for managed/remote hosts and
	// "disable" for local/docker Postgres (which terminates plaintext on the
	// container network). Production MUST run with sslmode=require (or stricter:
	// verify-ca / verify-full) — see .env.example.
	cfg.DatabaseURL = applySSLMode(cfg.DatabaseURL, os.Getenv("DB_SSLMODE"))

	return cfg, nil
}

// applySSLMode returns dsn with an sslmode parameter guaranteed to be present.
//
//   - If dsn already specifies sslmode, it is left untouched (operator intent
//     wins — including an explicit local sslmode=disable).
//   - Else if override (DB_SSLMODE) is non-empty, that value is used.
//   - Else the mode is inferred from the host: "disable" for local/docker
//     Postgres, "require" for everything else (managed/remote).
//
// Only URL-style DSNs (postgres://, postgresql://) are rewritten; key/value
// DSNs are returned unchanged with the override appended when supplied, so the
// caller's explicit configuration is never silently dropped.
func applySSLMode(dsn, override string) string {
	override = sanitizeSSLMode(override)

	u, err := url.Parse(dsn)
	if err != nil || (u.Scheme != "postgres" && u.Scheme != "postgresql") {
		// Not a URL DSN we can safely rewrite. Honor an explicit override by
		// appending it; otherwise leave the operator's DSN as-is.
		if override != "" && !strings.Contains(dsn, "sslmode") {
			sep := " "
			if strings.Contains(dsn, "://") {
				sep = "?"
				if strings.Contains(dsn, "?") {
					sep = "&"
				}
			}
			return dsn + sep + "sslmode=" + override
		}
		return dsn
	}

	q := u.Query()
	if q.Get("sslmode") != "" {
		// Operator already chose a mode — respect it.
		return dsn
	}

	mode := override
	if mode == "" {
		if isLocalHost(u.Hostname()) {
			mode = "disable"
		} else {
			mode = "require"
		}
	}
	q.Set("sslmode", mode)
	u.RawQuery = q.Encode()
	return u.String()
}

// sanitizeSSLMode validates a DB_SSLMODE override against the libpq-supported
// mode allowlist. An unrecognized value (typo, or an injection attempt like
// "require;DROP TABLE") is rejected and treated as unset, so it can never be
// concatenated verbatim into the DSN. Comparison is case-insensitive.
func sanitizeSSLMode(mode string) string {
	mode = strings.ToLower(strings.TrimSpace(mode))
	switch mode {
	case "disable", "allow", "prefer", "require", "verify-ca", "verify-full":
		return mode
	default:
		return ""
	}
}

// isLocalHost reports whether host is a loopback or a well-known in-cluster
// Postgres service name where TLS is not terminated. Used only to pick the
// default sslmode; an explicit DB_SSLMODE or an sslmode in the DSN overrides it.
func isLocalHost(host string) bool {
	host = strings.ToLower(strings.TrimSpace(host))
	switch host {
	case "localhost", "127.0.0.1", "::1", "":
		return true
	}
	// Docker-compose / k8s service names used for the local Postgres in this
	// monorepo. These never present a TLS endpoint, so requiring TLS would
	// break local bring-up.
	localServiceNames := []string{
		"controlplane-postgres",
		"postgres",
		"db",
	}
	for _, name := range localServiceNames {
		if host == name {
			return true
		}
	}
	return false
}

func getEnv(key, fallback string) string {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	return v
}

func getEnvInt(key string, fallback int) int {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	parsed := fallback
	_, _ = fmt.Sscanf(v, "%d", &parsed)
	return parsed
}

func getEnvBool(key string, fallback bool) bool {
	v := os.Getenv(key)
	if v == "" {
		return fallback
	}
	return v == "true" || v == "1" || v == "yes"
}
