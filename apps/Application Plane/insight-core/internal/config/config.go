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

	// W3 (PR-3), all OPTIONAL. When DatabaseURL is empty insight-core keeps the
	// in-memory metric repo (registry-only); when NATSURL is empty the
	// conversation/social metric subscriber is not started.
	DatabaseURL  string
	NATSURL      string
	NATSUser     string
	NATSPassword string

	// W3 (PR-5) — model-plane-agents producer leg, OPTIONAL. The Model Plane
	// runs on an ISOLATED NATS cluster (the model-plane bus), so the agent
	// subscriber dual-connects here. When empty the agents producer is not
	// started (mirrors notification-core's MODEL_PLANE_NATS_URL bridge).
	ModelPlaneNATSURL      string
	ModelPlaneNATSUser     string
	ModelPlaneNATSPassword string

	// SocialCoreURL is the metrics.snapshotted follow-up fetch target
	// (GET /api/v1/social/metrics) — social-core's lifecycle event carries
	// only a summary count, so the metric subscriber fetches the real values
	// here. Reuses InternalAPIKey (the same shared cross-plane key every
	// other core-to-core call in this repo uses). Empty disables the
	// external_analytics surface without affecting the count-based metrics.
	SocialCoreURL string
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
		DatabaseURL:                   strings.TrimSpace(getEnv("DATABASE_URL", "")),
		NATSURL:                       strings.TrimSpace(getEnv("NATS_URL", "")),
		NATSUser:                      strings.TrimSpace(getEnv("NATS_USER", "")),
		NATSPassword:                  strings.TrimSpace(getEnv("NATS_PASSWORD", "")),
		ModelPlaneNATSURL:             strings.TrimSpace(getEnv("MODEL_PLANE_NATS_URL", "")),
		ModelPlaneNATSUser:            strings.TrimSpace(getEnv("MODEL_PLANE_NATS_USER", "")),
		ModelPlaneNATSPassword:        strings.TrimSpace(getEnv("MODEL_PLANE_NATS_PASSWORD", "")),
		SocialCoreURL:                 strings.TrimRight(strings.TrimSpace(getEnv("SOCIAL_CORE_URL", "http://social-core:3162")), "/"),
	}

	if cfg.InternalAPIKey == "" {
		return nil, fmt.Errorf("INTERNAL_API_KEY is required")
	}
	if cfg.ConnectorTokenLeaseAudience == "" {
		return nil, fmt.Errorf("INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE is required")
	}
	if cfg.NATSURL != "" && (cfg.NATSUser == "" || len(cfg.NATSPassword) < 32) {
		return nil, fmt.Errorf("NATS_USER and NATS_PASSWORD are required for the Application Plane bus")
	}
	if cfg.ModelPlaneNATSURL != "" && (cfg.ModelPlaneNATSUser == "" || len(cfg.ModelPlaneNATSPassword) < 32) {
		return nil, fmt.Errorf("MODEL_PLANE_NATS_USER and MODEL_PLANE_NATS_PASSWORD are required for the Model Plane bus")
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
