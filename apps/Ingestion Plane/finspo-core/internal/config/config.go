package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	Port               string
	APIKey             string
	APIKeyHeader       string
	GraphBaseURL       string
	IntegrationCoreURL string
	InternalAPIKey     string

	Environment string
	ServiceName string

	DatabaseURL string

	NATSURL           string
	NATSSubjectPrefix string

	OTELExporterEndpoint string

	DataPlaneDocumentsURL string
	AuthCoreURL           string
	FinspoServiceID       string
	FinspoServiceAPIKey   string

	// Phase 3 — scheduler + permission capture knobs.
	SyncInterval       time.Duration
	CapturePermissions bool

	// Content capture — download each synced file's bytes, extract text, and
	// forward a content-bearing document to Data Plane v2. Defaults OFF: it is a
	// heavier operation (a download + extraction per file) with PII/cost
	// implications, so a deployment opts in explicitly. Requires
	// DataPlaneDocumentsURL plus the Auth Core service-principal settings to be
	// set as well.
	CaptureContent           bool
	ContentMaxBytes          int64
	ContentZDRClassification string

	// Phase 5 — governance execution knobs.
	// AllowExecution is a hard kill-switch for destructive Graph operations.
	// It defaults to FALSE: even an approved proposal cannot be executed until
	// an operator explicitly opts the deployment in. ArchiveFolderID is the
	// destination folder (within each item's own drive) for archive proposals.
	AllowExecution  bool
	ArchiveFolderID string
}

func Load() (Config, error) {
	apiKey := strings.TrimSpace(os.Getenv("FINSPO_API_KEY"))
	if apiKey == "" {
		return Config{}, fmt.Errorf("FINSPO_API_KEY is required")
	}

	port := envOr("PORT", "3130")
	apiKeyHeader := envOr("FINSPO_API_KEY_HEADER", "X-API-Key")
	graphBaseURL := envOr("GRAPH_BASE_URL", "https://graph.microsoft.com")

	integrationCoreURL := strings.TrimSpace(os.Getenv("INTEGRATION_CORE_URL"))
	if integrationCoreURL == "" {
		return Config{}, fmt.Errorf("INTEGRATION_CORE_URL is required")
	}

	internalAPIKey := strings.TrimSpace(os.Getenv("INTERNAL_API_KEY"))
	if internalAPIKey == "" {
		return Config{}, fmt.Errorf("INTERNAL_API_KEY is required")
	}

	databaseURL := strings.TrimSpace(os.Getenv("FINSPO_DSN"))
	if databaseURL == "" {
		return Config{}, fmt.Errorf("FINSPO_DSN is required")
	}
	dataPlaneURL := strings.TrimSpace(os.Getenv("DATA_PLANE_DOCUMENTS_BASE_URL"))
	authCoreURL := envOr("AUTH_CORE_URL", "http://auth-core:3011")
	finspoServiceID := envOr("FINSPO_SERVICE_ID", "finspo-core")
	finspoServiceAPIKey := strings.TrimSpace(os.Getenv("FINSPO_SERVICE_API_KEY"))
	if dataPlaneURL != "" {
		if authCoreURL == "" {
			return Config{}, fmt.Errorf("AUTH_CORE_URL is required when DATA_PLANE_DOCUMENTS_BASE_URL is set")
		}
		if finspoServiceID == "" {
			return Config{}, fmt.Errorf("FINSPO_SERVICE_ID is required when DATA_PLANE_DOCUMENTS_BASE_URL is set")
		}
		if finspoServiceAPIKey == "" {
			return Config{}, fmt.Errorf("FINSPO_SERVICE_API_KEY is required when DATA_PLANE_DOCUMENTS_BASE_URL is set")
		}
	}

	return Config{
		Port:                     port,
		APIKey:                   apiKey,
		APIKeyHeader:             apiKeyHeader,
		GraphBaseURL:             graphBaseURL,
		IntegrationCoreURL:       integrationCoreURL,
		InternalAPIKey:           internalAPIKey,
		Environment:              envOr("ENVIRONMENT", "dev"),
		ServiceName:              envOr("SERVICE_NAME", "finspo-core"),
		DatabaseURL:              databaseURL,
		NATSURL:                  strings.TrimSpace(os.Getenv("NATS_URL")),
		NATSSubjectPrefix:        envOr("NATS_SUBJECT_PREFIX", "finspo"),
		OTELExporterEndpoint:     strings.TrimSpace(os.Getenv("OTEL_EXPORTER_OTLP_ENDPOINT")),
		DataPlaneDocumentsURL:    dataPlaneURL,
		AuthCoreURL:              authCoreURL,
		FinspoServiceID:          finspoServiceID,
		FinspoServiceAPIKey:      finspoServiceAPIKey,
		SyncInterval:             envDuration("FINSPO_SYNC_INTERVAL", 5*time.Minute),
		CapturePermissions:       envBool("FINSPO_CAPTURE_PERMISSIONS", true),
		CaptureContent:           envBool("FINSPO_CAPTURE_CONTENT", false),
		ContentMaxBytes:          envInt64("FINSPO_CONTENT_MAX_BYTES", 0),
		ContentZDRClassification: envOr("FINSPO_CONTENT_ZDR_CLASSIFICATION", "internal"),
		AllowExecution:           envBool("FINSPO_ALLOW_EXECUTION", false),
		ArchiveFolderID:          strings.TrimSpace(os.Getenv("FINSPO_ARCHIVE_FOLDER_ID")),
	}, nil
}

func envInt64(key string, fallback int64) int64 {
	v := strings.TrimSpace(os.Getenv(key))
	if v == "" {
		return fallback
	}
	n, err := strconv.ParseInt(v, 10, 64)
	if err != nil || n < 0 {
		return fallback
	}
	return n
}

func envDuration(key string, fallback time.Duration) time.Duration {
	v := strings.TrimSpace(os.Getenv(key))
	if v == "" {
		return fallback
	}
	d, err := time.ParseDuration(v)
	if err != nil || d <= 0 {
		return fallback
	}
	return d
}

func envBool(key string, fallback bool) bool {
	v := strings.TrimSpace(os.Getenv(key))
	if v == "" {
		return fallback
	}
	b, err := strconv.ParseBool(v)
	if err != nil {
		return fallback
	}
	return b
}

func envOr(key, fallback string) string {
	v := strings.TrimSpace(os.Getenv(key))
	if v == "" {
		return fallback
	}
	return v
}
