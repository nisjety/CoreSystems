package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort                      int
	ServiceName                   string
	InternalAPIKey                string
	IntegrationCoreURL            string
	ConnectorTokenLeaseAudience   string
	GoogleAnalyticsAPIBaseURL     string
	GoogleSearchConsoleAPIBaseURL string
}

func Load() (*Config, error) {
	_ = godotenv.Load()

	cfg := &Config{
		HTTPPort:                      getEnvInt("PORT", 3163),
		ServiceName:                   strings.TrimSpace(getEnv("SERVICE_NAME", "insight-core")),
		InternalAPIKey:                strings.TrimSpace(getEnv("INTERNAL_API_KEY", "")),
		IntegrationCoreURL:            strings.TrimRight(strings.TrimSpace(getEnv("INTEGRATION_CORE_URL", "http://integration-api:3026")), "/"),
		ConnectorTokenLeaseAudience:   strings.TrimSpace(getEnv("INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE", "insight-core")),
		GoogleAnalyticsAPIBaseURL:     strings.TrimRight(strings.TrimSpace(getEnv("GOOGLE_ANALYTICS_DATA_API_BASE_URL", "https://analyticsdata.googleapis.com")), "/"),
		GoogleSearchConsoleAPIBaseURL: strings.TrimRight(strings.TrimSpace(getEnv("GOOGLE_SEARCH_CONSOLE_API_BASE_URL", "https://www.googleapis.com/webmasters/v3")), "/"),
	}

	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if cfg.ConnectorTokenLeaseAudience == "" {
		return nil, fmt.Errorf("INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE is required")
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
