package config

import (
	"fmt"
	"io"
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
	// only runs when enabled and the shared gateway's tenant credentials exist.
	IntegrationCoreURL                  string
	IntegrationCoreTenantBearerToken    string
	IntegrationCoreTenantOrganizationID string
	ProviderLeadSyncEnabled             bool
	ProviderLeadSyncInterval            time.Duration
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
		NATSPassword:   getEnv("NATS_PASSWORD", getEnv("STUDIO_LEADS_NATS_PASSWORD", getEnv("APPLICATION_LEADS_NATS_PASSWORD", ""))),
		IntegrationCoreURL: strings.TrimRight(
			getEnv("INTEGRATION_CORE_URL", "http://integration-api:3026"), "/"),
		IntegrationCoreTenantOrganizationID: strings.TrimSpace(os.Getenv("INTEGRATION_CORE_TENANT_ORGANIZATION_ID")),
		ProviderLeadSyncEnabled:             getEnvBool("PROVIDER_LEAD_SYNC_ENABLED", true),
		ProviderLeadSyncInterval:            getEnvDuration("PROVIDER_LEAD_SYNC_INTERVAL", time.Hour),
		BillingCoreURL:                      strings.TrimSpace(os.Getenv("BILLING_CORE_URL")),
		BillingServiceID:                    strings.TrimSpace(os.Getenv("LEADS_BILLING_SERVICE_ID")),
		BillingServiceToken:                 strings.TrimSpace(os.Getenv("LEADS_BILLING_SERVICE_TOKEN")),
	}
	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	var err error
	cfg.IntegrationCoreTenantBearerToken, err = readSecretFile("INTEGRATION_CORE_TENANT_BEARER_TOKEN_FILE")
	if err != nil {
		return nil, err
	}
	if cfg.IntegrationCoreTenantBearerToken != "" && cfg.IntegrationCoreTenantBearerToken == cfg.InternalAPIKey {
		return nil, fmt.Errorf("integration-core tenant bearer must differ from inbound INTERNAL_API_KEY")
	}
	if (cfg.IntegrationCoreTenantBearerToken == "") != (cfg.IntegrationCoreTenantOrganizationID == "") {
		return nil, fmt.Errorf("integration-core tenant bearer file and organization ID must be configured together")
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

// IntegrationReady prevents provider synchronization until the shared gateway
// and its organization-scoped credential are configured together.
func (c Config) IntegrationReady() bool {
	return strings.TrimSpace(c.IntegrationCoreURL) != "" && strings.TrimSpace(c.IntegrationCoreTenantBearerToken) != "" && strings.TrimSpace(c.IntegrationCoreTenantOrganizationID) != ""
}

func readSecretFile(envName string) (string, error) {
	const maxCredentialBytes = 8 << 10
	path := strings.TrimSpace(os.Getenv(envName))
	if path == "" {
		return "", nil
	}
	info, err := os.Stat(path)
	if err != nil {
		return "", fmt.Errorf("%s cannot be read: %w", envName, err)
	}
	if !info.Mode().IsRegular() || info.Size() > maxCredentialBytes {
		return "", fmt.Errorf("%s must reference a regular file of at most 8 KiB", envName)
	}
	file, err := os.Open(path)
	if err != nil {
		return "", fmt.Errorf("%s cannot be read: %w", envName, err)
	}
	defer file.Close()
	openedInfo, err := file.Stat()
	if err != nil || !openedInfo.Mode().IsRegular() || openedInfo.Size() > maxCredentialBytes {
		return "", fmt.Errorf("%s must reference a regular file of at most 8 KiB", envName)
	}
	raw, err := io.ReadAll(io.LimitReader(file, maxCredentialBytes+1))
	if err != nil {
		return "", fmt.Errorf("%s cannot be read: %w", envName, err)
	}
	if len(raw) > maxCredentialBytes {
		return "", fmt.Errorf("%s exceeds 8 KiB", envName)
	}
	secret := strings.TrimSpace(string(raw))
	if secret == "" || strings.ContainsAny(secret, " \t\r\n") {
		return "", fmt.Errorf("%s must contain one nonempty credential without whitespace", envName)
	}
	return secret, nil
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
