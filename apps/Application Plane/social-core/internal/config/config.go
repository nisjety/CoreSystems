package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort                  int
	DatabaseURL               string
	ServiceName               string
	InternalAPIKey            string
	IntegrationCoreURL        string
	NATSURL                   string
	NATSToken                 string
	PublishWorkerEnabled      bool
	PublishWorkerPollInterval time.Duration
	PublishWorkerBatchSize    int
	MetricsWorkerEnabled      bool
	MetricsWorkerPollInterval time.Duration
	LinkedInAPIBaseURL        string
	LinkedInAPIVersion        string
	XAPIBaseURL               string
	InstagramGraphAPIBaseURL  string
	FacebookGraphAPIBaseURL   string
	TikTokAPIBaseURL          string
	SnapchatAPIBaseURL        string
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
		HTTPPort:                  getEnvInt("PORT", 3162),
		DatabaseURL:               strings.TrimSpace(getEnv("DATABASE_URL", "")),
		ServiceName:               strings.TrimSpace(getEnv("SERVICE_NAME", "social-core")),
		InternalAPIKey:            strings.TrimSpace(getEnv("INTERNAL_API_KEY", "")),
		IntegrationCoreURL:        strings.TrimRight(strings.TrimSpace(getEnv("INTEGRATION_CORE_URL", "http://integration-api:3026")), "/"),
		NATSURL:                   strings.TrimSpace(getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", getEnv("NATS_URL", "nats://nats:4222")))),
		NATSToken:                 strings.TrimSpace(getEnv("VELION_NATS_TOKEN", getEnv("NATS_TOKEN", ""))),
		PublishWorkerEnabled:      getEnvBool("SOCIAL_PUBLISH_WORKER_ENABLED", true),
		PublishWorkerPollInterval: getEnvDuration("SOCIAL_PUBLISH_WORKER_POLL_INTERVAL", 5*time.Second),
		PublishWorkerBatchSize:    getEnvInt("SOCIAL_PUBLISH_WORKER_BATCH_SIZE", 10),
		MetricsWorkerEnabled:      getEnvBool("SOCIAL_METRICS_WORKER_ENABLED", true),
		MetricsWorkerPollInterval: getEnvDuration("SOCIAL_METRICS_WORKER_POLL_INTERVAL", 6*time.Hour),
		LinkedInAPIBaseURL:        strings.TrimRight(strings.TrimSpace(getEnv("LINKEDIN_API_BASE_URL", "https://api.linkedin.com")), "/"),
		LinkedInAPIVersion:        strings.TrimSpace(getEnv("LINKEDIN_API_VERSION", "202606")),
		XAPIBaseURL:               strings.TrimRight(strings.TrimSpace(getEnv("X_API_BASE_URL", "https://api.x.com")), "/"),
		InstagramGraphAPIBaseURL:  strings.TrimRight(strings.TrimSpace(getEnv("INSTAGRAM_GRAPH_API_BASE_URL", "https://graph.facebook.com/v25.0")), "/"),
		FacebookGraphAPIBaseURL:   strings.TrimRight(strings.TrimSpace(getEnv("FACEBOOK_GRAPH_API_BASE_URL", "https://graph.facebook.com/v25.0")), "/"),
		TikTokAPIBaseURL:          strings.TrimRight(strings.TrimSpace(getEnv("TIKTOK_API_BASE_URL", "https://open.tiktokapis.com")), "/"),
		SnapchatAPIBaseURL:        strings.TrimRight(strings.TrimSpace(getEnv("SNAPCHAT_API_BASE_URL", "https://adsapi.snapchat.com/v1")), "/"),
		GoogleAdsAPIBaseURL:       strings.TrimRight(strings.TrimSpace(getEnv("GOOGLE_ADS_API_BASE_URL", "https://googleads.googleapis.com/v24")), "/"),
		GoogleAdsDeveloperToken:   strings.TrimSpace(getEnv("GOOGLE_ADS_DEVELOPER_TOKEN", "")),
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if cfg.PublishWorkerBatchSize < 1 || cfg.PublishWorkerBatchSize > 100 {
		cfg.PublishWorkerBatchSize = 10
	}

	return cfg, nil
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
