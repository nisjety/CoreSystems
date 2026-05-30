package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
)

type Config struct {
	Port                         string
	AzureOpenAIEndpoint          string
	AzureOpenAIKey               string
	AzureOpenAIModel             string
	AICoreHTTPBaseURL            string
	AICoreInternalAPIKey         string // x-internal-api-key header for Model Plane v2 ai-core
	TemporalEnabled              bool
	TemporalAddress              string
	TemporalNamespace            string
	TemporalTaskQueue            string
	MaxConcurrentPages           int
	BrowserPoolSize              int
	UserAgent                    string
	EnableStealth                bool
	ScreenshotEnabled            bool
	BrowserServiceURL            string
	BrowserServiceInternalAPIKey string
	BrowserServiceTimeoutSec     int
	BrowserDevAllowInsecureTLS   bool
	BrowserDevDisableWebSecurity bool
	BraveSearchAPIKey            string
	BraveSearchBaseURL           string
	BraveSearchTimeoutSec        int
	GitHubAPIBaseURL             string
	GitHubToken                  string
	GitHubTimeoutSec             int
	RetrievalBaseURL             string
	DataplaneInternalAPIKey      string

	// Artifact storage configuration
	ArtifactStoreBackend string
	MinIOEndpoint        string
	MinIOAccessKey       string
	MinIOSecretKey       string
	MinIOBucket          string
	MinIOUseSSL          bool
	MinIORegion          string
	MinIOPublicBaseURL   string

	// Cache configuration
	CacheBackend      string // "memory", "disk", or "redis"
	CachePath         string // For disk cache
	DefaultMaxAgeMs   int64  // Default maxAge in milliseconds (default 172800000 = 2 days)
	MaxCacheSizeBytes int64  // Maximum cache size in bytes

	// Batch scraping configuration
	BatchMaxWorkers  int    // Maximum concurrent workers for batch jobs
	BatchJobTTLHours int    // Job retention time in hours (default 24)
	WebhookSecret    string // Secret for webhook signature verification
	JobStoreBackend  string // memory, redis, or postgres
	RedisURL         string // redis connection string
	PostgresDSN      string // postgres connection string for durable job persistence

	// Security provider configuration
	GoogleSafeBrowsingAPIKey string
	PhishTankAPIKey          string
	AbuseIPDBAPIKey          string
	SecurityBlockThreshold   float64

	// Phase 1.1 hardening
	APIKey               string
	APIKeyHeader         string
	RateLimitMax         int
	RateLimitWindowSec   int
	RequestTimeoutSec    int
	MaxRequestBodyBytes  int
	ScrapeMaxPages       int
	ScrapeMaxEnrichLimit int
	ScrapeMaxAgeMs       int64

	// Control plane integration
	AuthCoreURL            string // auth-core base URL (e.g. http://auth-core:3011)
	AuthCoreGRPCAddr       string // auth-core gRPC addr (e.g. auth-core:50011) — used for VerifyToken
	AuthCoreInternalAPIKey string // shared internal key for service-to-service calls
	BillingCoreURL         string // billing-core base URL (e.g. http://billing-core:3014)
	OrgCoreURL             string // org-core base URL (e.g. http://org-core:3020)
	UserCoreURL            string // user-core base URL (e.g. http://user-core:3021)
	ControlPlaneCacheSec   int    // cache ttl for account / entitlements lookups
	IdempotencyTTLHours    int    // retention for async idempotency records

	// Phase 2.1 AI integration
	EnableAIExtraction bool
	// AI extraction timeout in seconds (used for ai-core ExtractData calls)
	AIExtractionTimeoutSec int

	// Phase 4 middleware
	HumanDelayEnabled bool
	HumanDelayMinMs   int
	HumanDelayMaxMs   int
	ProxyEnabled      bool
	ProxyPool         []string
	RetryEnabled      bool
	RetryMaxAttempts  int
	RetryBackoffMs    int
	JobMonitoring     bool

	// NATS event publishing
	NATSSharedURL        string
	NATSSharedToken      string
	VelionNATSURL        string
	VelionNATSToken      string
	velionNATSConfigured bool

	// Research parallelism
	ResearchConcurrency int // Max concurrent search queries in deep research (default 3)
	ResearchScrapePara  int // Max concurrent source scrapes per round (default 5)

	// Agent configuration
	AgentTimeoutSec        int    // Default agent job timeout (default 120)
	AgentScrapeConcurrency int    // Max concurrent URL scrapes per agent job (default 5)
	AgentDefaultModel      string // Default model to use when not specified

	// Proprietary search index
	SearchIndexEnabled   bool
	SearchIndexPath      string
	SearchIndexBatchSize int

	// Email / OTP onboarding
	ResendAPIKey string // Resend.com API key (preferred)
	SMTPHost     string // SMTP host (fallback)
	SMTPPort     int    // SMTP port (default 587)
	SMTPFrom     string // From address used for OTP emails
	SMTPUser     string
	SMTPPass     string

	// SPA adaptive wait engine
	SPAWaitTimeoutMs       int // Overall SPA wait timeout in ms (default 15000)
	MutationQuietMs        int // MutationObserver quiet period in ms (default 500)
	ContentPlateauSamples  int // Consecutive equal-length samples to declare stable (default 3)
	ContentPlateauInterval int // Interval between plateau samples in ms (default 200)

	// Fingerprint rotation
	FingerprintRotation bool // Enable per-request fingerprint rotation (default true)

	// LLMs.txt
	LLMsTxtMaxPages int // Maximum pages in llmstxt output (default 100)
}

func Load() (*Config, error) {
	velionNATSURL, velionNATSURLSet := os.LookupEnv("VELION_NATS_URL")
	velionNATSToken, velionNATSTokenSet := os.LookupEnv("VELION_NATS_TOKEN")
	if err := validatePublisherOverrideEnv(velionNATSURL, velionNATSToken, velionNATSURLSet, velionNATSTokenSet); err != nil {
		return nil, err
	}

	cfg := &Config{
		Port:                         getEnv("PORT", "8090"),
		AzureOpenAIEndpoint:          getEnv("AZURE_OPENAI_ENDPOINT", ""),
		AzureOpenAIKey:               getEnv("AZURE_OPENAI_KEY", ""),
		AzureOpenAIModel:             getEnv("AZURE_OPENAI_MODEL", "gpt-4"),
		AICoreHTTPBaseURL:            getEnv("AI_CORE_HTTP_BASE_URL", "http://ai-core:8001"),
		AICoreInternalAPIKey:         getEnv("AI_CORE_INTERNAL_API_KEY", ""),
		TemporalEnabled:              getEnvBool("TEMPORAL_ENABLED", false),
		TemporalAddress:              getEnv("TEMPORAL_ADDRESS", "localhost:7233"),
		TemporalNamespace:            getEnv("TEMPORAL_NAMESPACE", "default"),
		TemporalTaskQueue:            getEnv("TEMPORAL_TASK_QUEUE", "quarry-task-queue"),
		MaxConcurrentPages:           getEnvInt("MAX_CONCURRENT_PAGES", 10),
		BrowserPoolSize:              getEnvInt("BROWSER_POOL_SIZE", 5),
		UserAgent:                    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36",
		EnableStealth:                true,
		ScreenshotEnabled:            false,
		BrowserServiceURL:            getEnv("BROWSER_SERVICE_URL", ""),
		BrowserServiceInternalAPIKey: getEnv("BROWSER_SERVICE_INTERNAL_API_KEY", ""),
		BrowserServiceTimeoutSec:     getEnvInt("BROWSER_SERVICE_TIMEOUT_SEC", 30),
		BrowserDevAllowInsecureTLS:   getEnvBool("BROWSER_DEV_ALLOW_INSECURE_TLS", false),
		BrowserDevDisableWebSecurity: getEnvBool("BROWSER_DEV_DISABLE_WEB_SECURITY", false),
		BraveSearchAPIKey:            getEnv("BRAVE_SEARCH_API_KEY", ""),
		BraveSearchBaseURL:           getEnv("BRAVE_SEARCH_BASE_URL", "https://api.search.brave.com"),
		BraveSearchTimeoutSec:        getEnvInt("BRAVE_SEARCH_TIMEOUT_SEC", 10),
		GitHubAPIBaseURL:             getEnv("GITHUB_API_BASE_URL", "https://api.github.com"),
		GitHubToken:                  getEnv("GITHUB_TOKEN", ""),
		GitHubTimeoutSec:             getEnvInt("GITHUB_TIMEOUT_SEC", 10),
		RetrievalBaseURL:             getEnv("RETRIEVAL_BASE_URL", ""),
		DataplaneInternalAPIKey:      getEnv("DATAPLANE_INTERNAL_API_KEY", getEnv("AUTH_CORE_INTERNAL_API_KEY", "")),
		ArtifactStoreBackend:         getEnv("ARTIFACT_STORE_BACKEND", ""),
		MinIOEndpoint:                getEnv("MINIO_ENDPOINT", ""),
		MinIOAccessKey:               getEnv("MINIO_ACCESS_KEY", ""),
		MinIOSecretKey:               getEnv("MINIO_SECRET_KEY", ""),
		MinIOBucket:                  getEnv("MINIO_BUCKET", "quarry-artifacts"),
		MinIOUseSSL:                  getEnvBool("MINIO_USE_SSL", false),
		MinIORegion:                  getEnv("MINIO_REGION", ""),
		MinIOPublicBaseURL:           getEnv("MINIO_PUBLIC_BASE_URL", ""),
		CacheBackend:                 getEnv("CACHE_BACKEND", "memory"),
		CachePath:                    getEnv("CACHE_PATH", "./data/cache"),
		DefaultMaxAgeMs:              172800000,  // 2 days
		MaxCacheSizeBytes:            1073741824, // 1GB
		BatchMaxWorkers:              getEnvInt("BATCH_MAX_WORKERS", 10),
		BatchJobTTLHours:             getEnvInt("BATCH_JOB_TTL_HOURS", 24),
		WebhookSecret:                getEnv("WEBHOOK_SECRET", ""),
		JobStoreBackend:              getEnv("JOB_STORE_BACKEND", "memory"),
		RedisURL:                     getEnv("REDIS_URL", ""),
		PostgresDSN:                  getEnv("QUARRY_POSTGRES_DSN", ""),
		GoogleSafeBrowsingAPIKey:     getEnv("GOOGLE_SAFE_BROWSING_API_KEY", ""),
		PhishTankAPIKey:              getEnv("PHISHTANK_API_KEY", ""),
		AbuseIPDBAPIKey:              getEnv("ABUSEIPDB_API_KEY", ""),
		SecurityBlockThreshold:       getEnvFloat64("SECURITY_BLOCK_THRESHOLD", 0.8),
		APIKey:                       getEnv("QUARRY_API_KEY", ""),
		APIKeyHeader:                 getEnv("QUARRY_API_KEY_HEADER", "X-API-Key"),
		RateLimitMax:                 getEnvInt("RATE_LIMIT_MAX", 60),
		RateLimitWindowSec:           getEnvInt("RATE_LIMIT_WINDOW_SEC", 60),
		RequestTimeoutSec:            getEnvInt("REQUEST_TIMEOUT_SEC", 30),
		MaxRequestBodyBytes:          getEnvInt("MAX_REQUEST_BODY_BYTES", 1048576),
		ScrapeMaxPages:               getEnvInt("SCRAPE_MAX_PAGES", 20),
		ScrapeMaxEnrichLimit:         getEnvInt("SCRAPE_MAX_ENRICH_LIMIT", 200),
		ScrapeMaxAgeMs:               getEnvInt64("SCRAPE_MAX_AGE_MS", 172800000),
		AuthCoreURL:                  getEnv("AUTH_CORE_URL", ""),
		AuthCoreGRPCAddr:             getEnv("AUTH_CORE_GRPC_ADDR", ""),
		AuthCoreInternalAPIKey:       getEnv("AUTH_CORE_INTERNAL_API_KEY", ""),
		BillingCoreURL:               getEnv("BILLING_CORE_URL", ""),
		OrgCoreURL:                   getEnv("ORG_CORE_URL", ""),
		UserCoreURL:                  getEnv("USER_CORE_URL", ""),
		ControlPlaneCacheSec:         getEnvInt("CONTROL_PLANE_CACHE_SEC", 60),
		IdempotencyTTLHours:          getEnvInt("IDEMPOTENCY_TTL_HOURS", 24),
		EnableAIExtraction:           getEnvBool("ENABLE_AI_EXTRACTION", true),
		AIExtractionTimeoutSec:       getEnvInt("AI_EXTRACTION_TIMEOUT_SEC", 60),
		HumanDelayEnabled:            getEnvBool("HUMAN_DELAY_ENABLED", false),
		HumanDelayMinMs:              getEnvInt("HUMAN_DELAY_MIN_MS", 25),
		HumanDelayMaxMs:              getEnvInt("HUMAN_DELAY_MAX_MS", 150),
		ProxyEnabled:                 getEnvBool("PROXY_ENABLED", false),
		ProxyPool:                    getEnvCSV("PROXY_POOL", ""),
		RetryEnabled:                 getEnvBool("RETRY_ENABLED", true),
		RetryMaxAttempts:             getEnvInt("RETRY_MAX_ATTEMPTS", 2),
		RetryBackoffMs:               getEnvInt("RETRY_BACKOFF_MS", 250),
		JobMonitoring:                getEnvBool("JOB_MONITORING_ENABLED", true),
		NATSSharedURL:                getEnv("NATS_SHARED_URL", ""),
		NATSSharedToken:              getEnv("NATS_SHARED_TOKEN", ""),
		VelionNATSURL:                velionNATSURL,
		VelionNATSToken:              velionNATSToken,
		velionNATSConfigured:         velionNATSURLSet && velionNATSTokenSet,
		ResearchConcurrency:          getEnvInt("RESEARCH_CONCURRENCY", 3),
		ResearchScrapePara:           getEnvInt("RESEARCH_SCRAPE_PARALLELISM", 5),
		AgentTimeoutSec:              getEnvInt("AGENT_TIMEOUT_SEC", 120),
		AgentScrapeConcurrency:       getEnvInt("AGENT_SCRAPE_CONCURRENCY", 5),
		AgentDefaultModel:            getEnv("AGENT_DEFAULT_MODEL", ""),
		SearchIndexEnabled:           getEnvBool("SEARCH_INDEX_ENABLED", false),
		SearchIndexPath:              getEnv("SEARCH_INDEX_PATH", "./data/searchindex"),
		SearchIndexBatchSize:         getEnvInt("SEARCH_INDEX_BATCH_SIZE", 100),
		ResendAPIKey:                 getEnv("RESEND_API_KEY", ""),
		SMTPHost:                     getEnv("SMTP_HOST", ""),
		SMTPPort:                     getEnvInt("SMTP_PORT", 587),
		SMTPFrom:                     getEnv("SMTP_FROM", ""),
		SMTPUser:                     getEnv("SMTP_USER", ""),
		SMTPPass:                     getEnv("SMTP_PASS", ""),
		SPAWaitTimeoutMs:             getEnvInt("SPA_WAIT_TIMEOUT_MS", 15000),
		MutationQuietMs:              getEnvInt("MUTATION_QUIET_MS", 500),
		ContentPlateauSamples:        getEnvInt("CONTENT_PLATEAU_SAMPLES", 3),
		ContentPlateauInterval:       getEnvInt("CONTENT_PLATEAU_INTERVAL_MS", 200),
		FingerprintRotation:          getEnvBool("FINGERPRINT_ROTATION", true),
		LLMsTxtMaxPages:              getEnvInt("LLMSTXT_MAX_PAGES", 100),
	}

	return cfg, nil
}


func validatePublisherOverrideEnv(velionURL, velionToken string, velionURLSet, velionTokenSet bool) error {
	if velionURLSet && !velionTokenSet {
		return fmt.Errorf("VELION_NATS_TOKEN must be set when VELION_NATS_URL is present")
	}
	if !velionURLSet && velionTokenSet {
		return fmt.Errorf("VELION_NATS_URL must be set when VELION_NATS_TOKEN is present")
	}
	if velionURLSet && strings.TrimSpace(velionURL) == "" {
		return fmt.Errorf("VELION_NATS_URL must be non-empty when publisher override is configured")
	}
	return nil
}

func (c *Config) PublisherNATSURL() string {
	if c == nil {
		return ""
	}
	if c.velionNATSConfigured {
		return c.VelionNATSURL
	}
	return c.NATSSharedURL
}

func (c *Config) PublisherNATSToken() string {
	if c == nil {
		return ""
	}
	if c.velionNATSConfigured {
		return c.VelionNATSToken
	}
	return c.NATSSharedToken
}

func getEnv(key, defaultValue string) string {
	if value := os.Getenv(key); value != "" {
		return value
	}
	return defaultValue
}

func getEnvInt(key string, defaultValue int) int {
	if value := os.Getenv(key); value != "" {
		if intVal, err := strconv.Atoi(value); err == nil {
			return intVal
		}
	}
	return defaultValue
}

func getEnvInt64(key string, defaultValue int64) int64 {
	if value := os.Getenv(key); value != "" {
		if intVal, err := strconv.ParseInt(value, 10, 64); err == nil {
			return intVal
		}
	}
	return defaultValue
}

func getEnvFloat64(key string, defaultValue float64) float64 {
	if value := os.Getenv(key); value != "" {
		if floatVal, err := strconv.ParseFloat(value, 64); err == nil {
			return floatVal
		}
	}
	return defaultValue
}

func getEnvBool(key string, defaultValue bool) bool {
	if value := os.Getenv(key); value != "" {
		return strings.EqualFold(value, "true") || value == "1"
	}
	return defaultValue
}

func getEnvCSV(key, defaultValue string) []string {
	raw := getEnv(key, defaultValue)
	if strings.TrimSpace(raw) == "" {
		return nil
	}

	parts := strings.Split(raw, ",")
	values := make([]string, 0, len(parts))
	for _, part := range parts {
		v := strings.TrimSpace(part)
		if v != "" {
			values = append(values, v)
		}
	}

	if len(values) == 0 {
		return nil
	}

	return values
}
