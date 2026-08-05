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
	NATSUser       string
	NATSPassword   string
	ServiceName    string
	DelegationKeys map[string]string
	NovuSecretKey  string // optional — activates real Novu delivery when set
	NovuBaseURL    string // optional — override for EU region
	DeliveryMode   string // "novu" or explicit fail-closed "disabled"

	// SharedNATSURL/SharedNATSUser/SharedNATSPassword configure a SECOND,
	// narrowly-scoped connection to the cross-plane control-shared-nats
	// broker (identity "notification-core-gdpr"), used only by the
	// org-deletion subscriber (internal/consumers/org_deletion_consumer.go).
	// Deliberately DISTINCT env var names from NATS_SHARED_URL, which this
	// deployment's docker-compose already points at the plane-local broker
	// (see docker-compose.yml comments "priority 1/2 in Go config chain") —
	// reusing that name here would be neutralized by that existing wiring.
	// Empty SharedNATSURL disables the org-deletion subscriber without
	// affecting the plane-local NATS client or any other consumer.
	SharedNATSURL      string
	SharedNATSUser     string
	SharedNATSPassword string
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:           getEnvInt("PORT", 3140),
		DatabaseURL:        strings.TrimSpace(getEnv("DATABASE_URL", "")),
		NATSURL:            strings.TrimSpace(getEnv("VEREVON_NATS_URL", getEnv("NATS_SHARED_URL", getEnv("NATS_URL", "nats://verevon-nats:4222")))),
		NATSUser:           strings.TrimSpace(getEnv("NATS_USER", "")),
		NATSPassword:       strings.TrimSpace(getEnv("NATS_PASSWORD", "")),
		ServiceName:        getEnv("SERVICE_NAME", "notification-core"),
		NovuSecretKey:      strings.TrimSpace(getEnv("NOVU_SECRET_KEY", "")),
		NovuBaseURL:        strings.TrimSpace(getEnv("NOVU_BASE_URL", "")),
		DeliveryMode:       strings.ToLower(strings.TrimSpace(getEnv("NOTIFICATION_DELIVERY_MODE", "disabled"))),
		SharedNATSURL:      strings.TrimSpace(getEnv("NOTIFICATION_GDPR_SHARED_NATS_URL", "")),
		SharedNATSUser:     strings.TrimSpace(getEnv("NOTIFICATION_GDPR_SHARED_NATS_USER", "")),
		SharedNATSPassword: strings.TrimSpace(getEnv("NOTIFICATION_GDPR_SHARED_NATS_PASSWORD", "")),
		DelegationKeys: map[string]string{
			"verevon-gateway": strings.TrimSpace(getEnv("NOTIFICATION_GATEWAY_SERVICE_TOKEN", "")),
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
	if !validDelegationSecret(keys["verevon-gateway"]) {
		return fmt.Errorf("NOTIFICATION_GATEWAY_SERVICE_TOKEN must be a non-placeholder secret of at least 32 bytes")
	}
	for serviceID, secret := range keys {
		if serviceID == "verevon-gateway" || strings.TrimSpace(secret) == "" {
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
