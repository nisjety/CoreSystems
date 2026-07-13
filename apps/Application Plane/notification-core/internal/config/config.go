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
	DelegationKeys map[string]string
	NovuSecretKey  string // optional — activates real Novu delivery when set
	NovuBaseURL    string // optional — override for EU region
	DeliveryMode   string // "novu" or explicit fail-closed "disabled"

	// G14: separate connection details for the **shared** velion-nats bus
	// where cross-plane events (app.session.*, etc.) flow. notification-core
	// keeps its local app-nats connection for its own publisher; the shared
	// bus is consumed read-only by subscribers/control_session.go.
	//
	// Defaults to the same NATSURL when SHARED_NATS_URL is unset (single-NATS
	// deployments).
	SharedNATSURL   string
	SharedNATSToken string
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:        getEnvInt("PORT", 3140),
		DatabaseURL:     strings.TrimSpace(getEnv("DATABASE_URL", "")),
		NATSURL:         strings.TrimSpace(getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", getEnv("NATS_URL", "nats://velion-nats:4222")))),
		NATSToken:       strings.TrimSpace(getEnv("VELION_NATS_TOKEN", getEnv("NATS_SHARED_TOKEN", getEnv("NATS_TOKEN", "")))),
		SharedNATSURL:   strings.TrimSpace(getEnv("SHARED_NATS_URL", "")),
		SharedNATSToken: strings.TrimSpace(getEnv("SHARED_NATS_TOKEN", "")),
		ServiceName:     getEnv("SERVICE_NAME", "notification-core"),
		NovuSecretKey:   strings.TrimSpace(getEnv("NOVU_SECRET_KEY", "")),
		NovuBaseURL:     strings.TrimSpace(getEnv("NOVU_BASE_URL", "")),
		DeliveryMode:    strings.ToLower(strings.TrimSpace(getEnv("NOTIFICATION_DELIVERY_MODE", "disabled"))),
		DelegationKeys: map[string]string{
			"velion-gateway": strings.TrimSpace(getEnv("NOTIFICATION_GATEWAY_SERVICE_TOKEN", "")),
			"support-worker": strings.TrimSpace(getEnv("NOTIFICATION_SUPPORT_WORKER_SERVICE_TOKEN", "")),
		},
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	if err := validateDelegationKeys(cfg.DelegationKeys); err != nil {
		return nil, err
	}
	for serviceID, secret := range cfg.DelegationKeys {
		if strings.TrimSpace(secret) == "" {
			delete(cfg.DelegationKeys, serviceID)
		}
	}
	switch cfg.DeliveryMode {
	case "disabled":
	case "novu":
		if cfg.NovuSecretKey == "" {
			return nil, fmt.Errorf("NOVU_SECRET_KEY is required when NOTIFICATION_DELIVERY_MODE=novu")
		}
	default:
		return nil, fmt.Errorf("NOTIFICATION_DELIVERY_MODE must be novu or disabled")
	}

	return cfg, nil
}

func validateDelegationKeys(keys map[string]string) error {
	if !validDelegationSecret(keys["velion-gateway"]) {
		return fmt.Errorf("NOTIFICATION_GATEWAY_SERVICE_TOKEN must be a non-placeholder secret of at least 32 bytes")
	}
	for serviceID, secret := range keys {
		if serviceID == "velion-gateway" || strings.TrimSpace(secret) == "" {
			continue
		}
		if !validDelegationSecret(secret) {
			return fmt.Errorf("delegation token for %s must be a non-placeholder secret of at least 32 bytes", serviceID)
		}
	}
	return nil
}

func validDelegationSecret(secret string) bool {
	secret = strings.TrimSpace(secret)
	lower := strings.ToLower(secret)
	return len(secret) >= 32 &&
		!strings.HasPrefix(lower, "change-me") &&
		!strings.HasPrefix(lower, "replace-with")
}

func getEnv(key, fallback string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}

	return fallback
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
