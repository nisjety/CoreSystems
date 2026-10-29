package config

import (
	"fmt"
	"io"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort                            int
	DatabaseURL                         string
	ServiceName                         string
	InternalAPIKey                      string
	IntegrationCoreURL                  string
	IntegrationCoreTenantBearerToken    string
	IntegrationCoreTenantOrganizationID string
	NATSURL                             string
	NATSUser                            string
	NATSPassword                        string
	PublishWorkerEnabled                bool
	PublishWorkerPollInterval           time.Duration
	PublishWorkerBatchSize              int
	MetricsWorkerEnabled                bool
	MetricsWorkerPollInterval           time.Duration
	LinkedInAPIBaseURL                  string
	LinkedInAPIVersion                  string
	XAPIBaseURL                         string
	InstagramGraphAPIBaseURL            string
	FacebookGraphAPIBaseURL             string
	TikTokAPIBaseURL                    string
	SnapchatAPIBaseURL                  string
	// SnapchatBusinessAPIBaseURL is the Public Profile API host (organic
	// Story/Spotlight posting); SnapchatLivePublishing gates real posting to it
	// (default off — allowlist-only vendor API, needs credentials).
	SnapchatBusinessAPIBaseURL string
	SnapchatLivePublishing     bool
	// Ads system wiring. Meta ads ride the same Graph API base as Pages; the
	// Marketing API is versioned in lockstep with Graph. Google Ads API calls
	// additionally require a developer token (`developer-token` header, from a
	// Google Ads manager account's API Center) on top of the OAuth adwords
	// scope — campaign workflows stay disabled until it is supplied.
	GoogleAdsAPIBaseURL     string
	GoogleAdsDeveloperToken string
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:                            getEnvInt("PORT", 3162),
		DatabaseURL:                         strings.TrimSpace(getEnv("DATABASE_URL", "")),
		ServiceName:                         strings.TrimSpace(getEnv("SERVICE_NAME", "social-core")),
		InternalAPIKey:                      strings.TrimSpace(getEnv("INTERNAL_API_KEY", "")),
		IntegrationCoreURL:                  strings.TrimRight(strings.TrimSpace(getEnv("INTEGRATION_CORE_URL", "http://integration-api:3026")), "/"),
		IntegrationCoreTenantOrganizationID: strings.TrimSpace(os.Getenv("INTEGRATION_CORE_TENANT_ORGANIZATION_ID")),
		NATSURL:                             strings.TrimSpace(getEnv("VEREVON_NATS_URL", getEnv("NATS_SHARED_URL", getEnv("NATS_URL", "nats://nats:4222")))),
		NATSUser:                            strings.TrimSpace(getEnv("NATS_USER", "")),
		NATSPassword:                        getEnv("NATS_PASSWORD", getEnv("STUDIO_SOCIAL_NATS_PASSWORD", getEnv("APPLICATION_SOCIAL_NATS_PASSWORD", ""))),
		PublishWorkerEnabled:                getEnvBool("SOCIAL_PUBLISH_WORKER_ENABLED", true),
		PublishWorkerPollInterval:           getEnvDuration("SOCIAL_PUBLISH_WORKER_POLL_INTERVAL", 5*time.Second),
		PublishWorkerBatchSize:              getEnvInt("SOCIAL_PUBLISH_WORKER_BATCH_SIZE", 10),
		MetricsWorkerEnabled:                getEnvBool("SOCIAL_METRICS_WORKER_ENABLED", true),
		MetricsWorkerPollInterval:           getEnvDuration("SOCIAL_METRICS_WORKER_POLL_INTERVAL", 6*time.Hour),
		LinkedInAPIBaseURL:                  strings.TrimRight(strings.TrimSpace(getEnv("LINKEDIN_API_BASE_URL", "https://api.linkedin.com")), "/"),
		LinkedInAPIVersion:                  strings.TrimSpace(getEnv("LINKEDIN_API_VERSION", "202606")),
		XAPIBaseURL:                         strings.TrimRight(strings.TrimSpace(getEnv("X_API_BASE_URL", "https://api.x.com")), "/"),
		InstagramGraphAPIBaseURL:            strings.TrimRight(strings.TrimSpace(getEnv("INSTAGRAM_GRAPH_API_BASE_URL", "https://graph.facebook.com/v25.0")), "/"),
		FacebookGraphAPIBaseURL:             strings.TrimRight(strings.TrimSpace(getEnv("FACEBOOK_GRAPH_API_BASE_URL", "https://graph.facebook.com/v25.0")), "/"),
		TikTokAPIBaseURL:                    strings.TrimRight(strings.TrimSpace(getEnv("TIKTOK_API_BASE_URL", "https://open.tiktokapis.com")), "/"),
		SnapchatAPIBaseURL:                  strings.TrimRight(strings.TrimSpace(getEnv("SNAPCHAT_API_BASE_URL", "https://adsapi.snapchat.com/v1")), "/"),
		SnapchatBusinessAPIBaseURL:          strings.TrimRight(strings.TrimSpace(getEnv("SNAPCHAT_BUSINESS_API_BASE_URL", "https://businessapi.snapchat.com/v1")), "/"),
		SnapchatLivePublishing:              getEnvBool("SNAPCHAT_LIVE_PUBLISHING", false),
		GoogleAdsAPIBaseURL:                 strings.TrimRight(strings.TrimSpace(getEnv("GOOGLE_ADS_API_BASE_URL", "https://googleads.googleapis.com/v24")), "/"),
		GoogleAdsDeveloperToken:             strings.TrimSpace(getEnv("GOOGLE_ADS_DEVELOPER_TOKEN", "")),
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
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
	if cfg.PublishWorkerBatchSize < 1 || cfg.PublishWorkerBatchSize > 100 {
		cfg.PublishWorkerBatchSize = 10
	}

	return cfg, nil
}

// IntegrationReady requires the shared gateway and its organization-scoped
// credential. The inbound Social API key never authorizes provider requests.
func (c Config) IntegrationReady() bool {
	return strings.TrimSpace(c.IntegrationCoreURL) != "" && strings.TrimSpace(c.IntegrationCoreTenantBearerToken) != "" && strings.TrimSpace(c.IntegrationCoreTenantOrganizationID) != ""
}

// PublishWorkerReady preserves queued posts until the governed executor is
// configured, instead of failing them because a gateway credential is absent.
func (c Config) PublishWorkerReady() bool {
	return c.PublishWorkerEnabled && c.IntegrationReady()
}

// MetricsWorkerReady keeps the background provider reader within one tenant.
func (c Config) MetricsWorkerReady() bool {
	return c.MetricsWorkerEnabled && c.IntegrationReady()
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

func getEnvBool(key string, fallback bool) bool {
	value := strings.ToLower(strings.TrimSpace(os.Getenv(key)))
	if value == "" {
		return fallback
	}
	switch value {
	case "1", "true", "yes", "on":
		return true
	case "0", "false", "no", "off":
		return false
	default:
		return fallback
	}
}

func getEnvDuration(key string, fallback time.Duration) time.Duration {
	value := strings.TrimSpace(os.Getenv(key))
	if value == "" {
		return fallback
	}
	parsed, err := time.ParseDuration(value)
	if err != nil || parsed <= 0 {
		return fallback
	}
	return parsed
}
