package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	HTTPPort       int
	ServiceName    string
	InternalAPIKey string
	DatabaseURL    string
	// Optional: when set, per-export audit events are published to NATS
	// (best-effort). Empty disables audit publishing (export still works).
	NATSURL      string
	NATSUser     string
	NATSPassword string
	// Provider lead sync (LinkedIn Lead Gen forms via integration-corev2's
	// actions gateway). The default base URL mirrors social-core's; the worker
	// only runs when enabled AND the URL is non-empty.
	IntegrationCoreURL       string
	ProviderLeadSyncEnabled  bool
	ProviderLeadSyncInterval time.Duration
	// Billing Core owns plan and entitlement decisions. Leads Core uses this
	// service principal only for the resource-server `leads` gate.
	BillingCoreURL      string
	BillingServiceID    string
	BillingServiceToken string
}

func Load() (*Config, error) {
	cfg := &Config{
		HTTPPort:       getEnvInt("PORT", 3164),
		ServiceName:    getEnv("SERVICE_NAME", "leads-core"),
		InternalAPIKey: strings.TrimSpace(os.Getenv("INTERNAL_API_KEY")),
		DatabaseURL:    strings.TrimSpace(os.Getenv("DATABASE_URL")),
		NATSURL:        strings.TrimSpace(os.Getenv("NATS_URL")),
		NATSUser:       strings.TrimSpace(os.Getenv("NATS_USER")),
		NATSPassword:   strings.TrimSpace(os.Getenv("NATS_PASSWORD")),
		IntegrationCoreURL: strings.TrimRight(
			getEnv("INTEGRATION_CORE_URL", "http://integration-api:3026"), "/"),
		ProviderLeadSyncEnabled:  getEnvBool("PROVIDER_LEAD_SYNC_ENABLED", true),
		ProviderLeadSyncInterval: getEnvDuration("PROVIDER_LEAD_SYNC_INTERVAL", time.Hour),
		BillingCoreURL:           strings.TrimSpace(os.Getenv("BILLING_CORE_URL")),
		BillingServiceID:         strings.TrimSpace(os.Getenv("LEADS_BILLING_SERVICE_ID")),
		BillingServiceToken:      strings.TrimSpace(os.Getenv("LEADS_BILLING_SERVICE_TOKEN")),
	}
	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	billingConfigured := 0
	for _, value := range []string{cfg.BillingCoreURL, cfg.BillingServiceID, cfg.BillingServiceToken} {
		if value != "" {
			billingConfigured++
		}
	}
	if billingConfigured != 0 && billingConfigured != 3 {
		return nil, fmt.Errorf("BILLING_CORE_URL, LEADS_BILLING_SERVICE_ID, and LEADS_BILLING_SERVICE_TOKEN must be configured together")
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

func getEnvBool(key string, fallback bool) bool {
	switch strings.ToLower(strings.TrimSpace(os.Getenv(key))) {
	case "1", "true", "yes", "on":
		return true
	case "0", "false", "no", "off":
		return false
	default:
		return fallback
	}
}

func getEnvDuration(key string, fallback time.Duration) time.Duration {
	v := strings.TrimSpace(os.Getenv(key))
	if v == "" {
		return fallback
	}
	parsed, err := time.ParseDuration(v)
	if err != nil || parsed <= 0 {
		return fallback
	}
	return parsed
}
