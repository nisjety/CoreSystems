package config

import (
	"fmt"
	"os"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort                  int
	GRPCPort                  int
	DatabaseURL               string
	NATSURL                   string
	NATSToken                 string
	NATSSharedURL             string
	NATSSharedToken           string
	ServiceName               string
	Redis                     RedisConfig
	PaymentProvider           string
	StripeBaseURL             string
	StripeAPIKey              string
	HyperswitchBaseURL        string
	HyperswitchAPIKey         string
	HyperswitchPublishableKey string
	HyperswitchProfileID      string
	HyperswitchClientURL      string
	HyperswitchBackendURL     string
	LagoBaseURL               string
	LagoAPIKey                string
	AdapterTimeoutSeconds     int
	RetryPollSeconds          int
	RetryBatchSize            int
	RetryMaxAttempts          int
	RetryBackoffSeconds       int
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
		HTTPPort:        getEnvInt("HTTP_PORT", 3014),
		GRPCPort:        getEnvInt("GRPC_PORT", 50013),
		DatabaseURL:     getEnv("DATABASE_URL", "postgres://aquatiq:change-me-db-password@controlplane-postgres:5432/postgres?sslmode=disable"),
		NATSURL:         getEnv("NATS_URL", "nats://controlplane-nats:4222"),
		NATSToken:       getEnv("NATS_TOKEN", getEnv("NATS_AUTH_TOKEN", "")),
		NATSSharedURL:   getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", "")),
		NATSSharedToken: getEnv("VELION_NATS_TOKEN", getEnv("NATS_SHARED_TOKEN", "")),
		ServiceName:     getEnv("SERVICE_NAME", "billing-core"),
		Redis: RedisConfig{
			Host:     getEnv("DRAGONFLY_HOST", getEnv("CACHE_HOST", getEnv("REDIS_HOST", "controlplane-dragonfly"))),
			Port:     getEnv("DRAGONFLY_PORT", getEnv("CACHE_PORT", getEnv("REDIS_PORT", "6379"))),
			Password: getEnv("DRAGONFLY_PASSWORD", getEnv("CACHE_PASSWORD", getEnv("REDIS_PASSWORD", ""))),
			DB:       getEnvInt("DRAGONFLY_DB", getEnvInt("CACHE_DB", getEnvInt("REDIS_DB", 5))),
			Enabled:  getEnvBool("DRAGONFLY_ENABLED", getEnvBool("CACHE_ENABLED", getEnvBool("REDIS_ENABLED", false))),
		},
		PaymentProvider:           getEnv("PAYMENT_PROVIDER", ""),
		StripeBaseURL:             getEnv("STRIPE_BASE_URL", "https://api.stripe.com"),
		StripeAPIKey:              getEnv("STRIPE_API_KEY_SECRET", getEnv("STRIPE_API_KEY", "")), // prefer secret key
		HyperswitchBaseURL:        getEnv("HYPERSWITCH_BASE_URL", "https://sandbox.hyperswitch.io"),
		HyperswitchAPIKey:         getEnv("HYPERSWITCH_API_KEY_SECRET", getEnv("HYPERSWITCH_API_KEY", "")),
		HyperswitchPublishableKey: getEnv("HYPERSWITCH_PUBLISHABLE_KEY", ""),
		HyperswitchProfileID:      getEnv("HYPERSWITCH_PROFILE_ID", ""),
		HyperswitchClientURL:      getEnv("HYPERSWITCH_CLIENT_URL", "https://beta.hyperswitch.io/v1/HyperLoader.js"),
		HyperswitchBackendURL:     getEnv("HYPERSWITCH_BACKEND_URL", getEnv("HYPERSWITCH_BASE_URL", "https://sandbox.hyperswitch.io")),
		LagoBaseURL:               getEnv("LAGO_BASE_URL", "http://lago:3000"),
		LagoAPIKey:                getEnv("LAGO_API_KEY", ""),
		AdapterTimeoutSeconds:     getEnvInt("ADAPTER_TIMEOUT_SECONDS", 10),
		RetryPollSeconds:          getEnvInt("RETRY_POLL_SECONDS", 5),
		RetryBatchSize:            getEnvInt("RETRY_BATCH_SIZE", 50),
		RetryMaxAttempts:          getEnvInt("RETRY_MAX_ATTEMPTS", 8),
		RetryBackoffSeconds:       getEnvInt("RETRY_BACKOFF_SECONDS", 5),
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}

	return cfg, nil
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
