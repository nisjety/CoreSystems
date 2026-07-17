package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

type Config struct {
	DatabaseURL                string
	NatsURL                    string
	NatsToken                  string
	HTTPPort                   int
	GRPCPort                   int
	SharedNatsURL              string
	SharedNatsUser             string
	SharedNatsPassword         string
	GDPRConsumerRequired       bool
	UserCoreServiceToken       string
	// UserCoreGrantsRequired keeps production fail-closed while allowing an
	// explicit standalone boot posture. When false, grant-only documents remain
	// hidden until User Core is connected; owner/org-visible policy is unchanged.
	UserCoreGrantsRequired     bool
	EventSigningPrivateKeyPath string
	// UserCoreURL is the user-core base URL used to resolve a viewer's explicit
	// resource grants (the per-user authz facade). Per-user ownership filtering.
	UserCoreURL string
}

func Load() (*Config, error) {
	cfg := &Config{
		DatabaseURL:                envOr("DATABASE_URL", "postgres://dataplane:dataplane@localhost:5442/dataplane?sslmode=disable"),
		NatsURL:                    envOr("NATS_URL", "nats://localhost:4232"),
		NatsToken:                  strings.TrimSpace(os.Getenv("DATAPLANE_NATS_TOKEN")),
		HTTPPort:                   envIntOr("HTTP_PORT", 8010),
		GRPCPort:                   envIntOr("GRPC_PORT", 50060),
		SharedNatsURL:              strings.TrimSpace(os.Getenv("NATS_SHARED_URL")),
		SharedNatsUser:             strings.TrimSpace(os.Getenv("NATS_SHARED_USER")),
		SharedNatsPassword:         strings.TrimSpace(os.Getenv("NATS_SHARED_PASSWORD")),
		GDPRConsumerRequired:       os.Getenv("GDPR_DURABLE_CONSUMER_REQUIRED") == "1",
		UserCoreServiceToken:       envOr("USER_CORE_SERVICE_TOKEN", ""),
		UserCoreGrantsRequired:     envOr("USER_CORE_GRANTS_REQUIRED", "1") != "0",
		EventSigningPrivateKeyPath: envOr("EVENT_SIGNING_PRIVATE_KEY_PATH", ""),
		UserCoreURL:                envOr("USER_CORE_URL", "http://user-core:8080"),
	}
	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL required")
	}
	if cfg.EventSigningPrivateKeyPath == "" {
		return nil, fmt.Errorf("EVENT_SIGNING_PRIVATE_KEY_PATH required")
	}
	sharedConfigured := cfg.SharedNatsURL != "" || cfg.SharedNatsUser != "" || cfg.SharedNatsPassword != ""
	if sharedConfigured && (cfg.SharedNatsURL == "" || cfg.SharedNatsUser == "" || cfg.SharedNatsPassword == "") {
		return nil, fmt.Errorf("NATS_SHARED_URL, NATS_SHARED_USER, and NATS_SHARED_PASSWORD must be configured together")
	}
	if cfg.SharedNatsPassword != "" && len(cfg.SharedNatsPassword) < 32 {
		return nil, fmt.Errorf("NATS_SHARED_PASSWORD must contain at least 32 characters")
	}
	if cfg.GDPRConsumerRequired && !sharedConfigured {
		return nil, fmt.Errorf("scoped shared NATS credentials are required for the durable GDPR consumer")
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
