package config

import (
	"fmt"
	"os"
	"strings"

	"github.com/joho/godotenv"
)

type Config struct {
	HTTPPort                     int
	GRPCPort                     int
	DatabaseURL                  string
	NATSURL                      string
	NATSToken                    string
	NATSSharedURL                string
	NATSSharedUser               string
	NATSSharedPass               string
	NATSSharedToken              string
	NATSSharedAllowTokenFallback bool
	ServiceName                  string
	OrgCoreServiceToken          string
	Redis                        RedisConfig
	PaymentProvider              string
	StripeBaseURL                string
	StripeAPIKey                 string
	HyperswitchBaseURL           string
	HyperswitchAPIKey            string
	HyperswitchPublishableKey    string
	HyperswitchProfileID         string
	HyperswitchClientURL         string
	HyperswitchBackendURL        string
	NexiEnvironment              string
	NexiBaseURL                  string
	NexiSecretKey                string
	NexiCheckoutKey              string
	NexiCheckoutJSURL            string
	NexiWebhookURL               string
	NexiWebhookAuthorization     string
	NexiTermsURL                 string
	LagoBaseURL                  string
	LagoAPIKey                   string
	AdapterTimeoutSeconds        int
	RetryPollSeconds             int
	RetryBatchSize               int
	RetryMaxAttempts             int
	RetryBackoffSeconds          int
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

	// Nexi: derive the API + Checkout JS base from NEXI_ENVIRONMENT (test|live),
	// mirroring the proven nettbutikk client (client.ts). Explicit NEXI_BASE_URL /
	// NEXI_CHECKOUT_JS_URL still override when set.
	nexiEnv := strings.ToLower(strings.TrimSpace(getEnv("NEXI_ENVIRONMENT", "test")))
	nexiAPIBase := "https://test.api.dibspayment.eu"
	nexiCheckoutJS := "https://test.checkout.dibspayment.eu/v1/checkout.js?v=1"
	if nexiEnv == "live" {
		nexiAPIBase = "https://api.dibspayment.eu"
		nexiCheckoutJS = "https://checkout.dibspayment.eu/v1/checkout.js?v=1"
	}

	cfg := &Config{
		HTTPPort:                     getEnvInt("HTTP_PORT", 3014),
		GRPCPort:                     getEnvInt("GRPC_PORT", 50013),
		DatabaseURL:                  getEnv("DATABASE_URL", "postgres://aquatiq:CHANGE_ME_set_DATABASE_URL@controlplane-postgres:5432/postgres?sslmode=disable"),
		NATSURL:                      getEnv("NATS_URL", "nats://controlplane-nats:4222"),
		NATSToken:                    getEnv("NATS_TOKEN", getEnv("NATS_AUTH_TOKEN", "")),
		NATSSharedURL:                getEnv("VELION_NATS_URL", getEnv("NATS_SHARED_URL", "")),
		NATSSharedUser:               getEnv("NATS_SHARED_USER", ""),
		NATSSharedPass:               getEnv("NATS_SHARED_PASSWORD", ""),
		NATSSharedToken:              getEnv("NATS_SHARED_TOKEN", ""),
		NATSSharedAllowTokenFallback: getEnvBool("NATS_SHARED_ALLOW_TOKEN_FALLBACK", false),
		ServiceName:                  getEnv("SERVICE_NAME", "billing-core"),
		OrgCoreServiceToken:          strings.TrimSpace(os.Getenv("ORG_CORE_SERVICE_TOKEN")),
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
		// Nexi Checkout (Nets/Nexi Group). Env var names match the proven
		// nettbutikk integration: NEXI_SECRET_KEY (server Authorization value, no
		// scheme), NEXI_CHECKOUT_KEY (public browser key for the Checkout JS SDK),
		// NEXI_ENVIRONMENT (test|live) drives the base URLs, NEXI_WEBHOOK_SECRET is
		// the shared secret Nexi echoes back in the webhook Authorization header.
		// The *_API_KEY / *_AUTHORIZATION names are kept as fallbacks for back-compat.
		NexiEnvironment:          nexiEnv,
		NexiBaseURL:              getEnv("NEXI_BASE_URL", nexiAPIBase),
		NexiSecretKey:            getEnv("NEXI_SECRET_KEY", getEnv("NEXI_SECRET_API_KEY", "")),
		NexiCheckoutKey:          getEnv("NEXI_CHECKOUT_KEY", ""),
		NexiCheckoutJSURL:        getEnv("NEXI_CHECKOUT_JS_URL", nexiCheckoutJS),
		NexiWebhookURL:           getEnv("NEXI_WEBHOOK_URL", ""),
		NexiWebhookAuthorization: getEnv("NEXI_WEBHOOK_SECRET", getEnv("NEXI_WEBHOOK_AUTHORIZATION", "")),
		NexiTermsURL:             getEnv("NEXI_TERMS_URL", ""),
		LagoBaseURL:              getEnv("LAGO_BASE_URL", "http://lago:3000"),
		LagoAPIKey:               getEnv("LAGO_API_KEY", ""),
		AdapterTimeoutSeconds:    getEnvInt("ADAPTER_TIMEOUT_SECONDS", 10),
		RetryPollSeconds:         getEnvInt("RETRY_POLL_SECONDS", 5),
		RetryBatchSize:           getEnvInt("RETRY_BATCH_SIZE", 50),
		RetryMaxAttempts:         getEnvInt("RETRY_MAX_ATTEMPTS", 8),
		RetryBackoffSeconds:      getEnvInt("RETRY_BACKOFF_SECONDS", 5),
	}

	if cfg.DatabaseURL == "" {
		return nil, fmt.Errorf("DATABASE_URL is required")
	}
	if !validDedicatedServiceToken(cfg.OrgCoreServiceToken) {
		return nil, fmt.Errorf("ORG_CORE_SERVICE_TOKEN must be a non-placeholder secret of at least 32 bytes")
	}
	for _, legacyName := range []string{"INTERNAL_API_KEY", "INTERNAL_SERVICE_SECRET"} {
		if legacy := strings.TrimSpace(os.Getenv(legacyName)); legacy != "" && legacy == cfg.OrgCoreServiceToken {
			return nil, fmt.Errorf("ORG_CORE_SERVICE_TOKEN must be distinct from legacy shared credentials")
		}
	}

	return cfg, nil
}

func validDedicatedServiceToken(token string) bool {
	token = strings.TrimSpace(token)
	lower := strings.ToLower(token)
	return len(token) >= 32 &&
		!strings.HasPrefix(lower, "test") &&
		!strings.HasPrefix(lower, "placeholder") &&
		!strings.HasPrefix(lower, "change-me") &&
		!strings.HasPrefix(lower, "replace-with")
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
