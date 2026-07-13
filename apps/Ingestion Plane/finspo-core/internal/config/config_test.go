package config

import "testing"

func setBaseEnv(t *testing.T) {
	t.Helper()
	t.Setenv("FINSPO_API_KEY", "test-key")
	t.Setenv("INTEGRATION_CORE_URL", "http://localhost:9000")
	t.Setenv("INTERNAL_API_KEY", "test-internal-key")
	t.Setenv("FINSPO_DSN", "postgres://finspo:finspo@localhost:5432/finspo?sslmode=disable")
}

func TestLoadFailsWhenAPIKeyMissing(t *testing.T) {
	t.Setenv("FINSPO_API_KEY", "")
	t.Setenv("PORT", "")
	t.Setenv("FINSPO_API_KEY_HEADER", "")

	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want non-nil")
	}
}

func TestLoadFailsWhenDSNMissing(t *testing.T) {
	setBaseEnv(t)
	t.Setenv("FINSPO_DSN", "")

	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want non-nil DSN required")
	}
}

func TestLoadAppliesDefaults(t *testing.T) {
	setBaseEnv(t)
	t.Setenv("PORT", "")
	t.Setenv("FINSPO_API_KEY_HEADER", "")
	t.Setenv("ENVIRONMENT", "")
	t.Setenv("NATS_SUBJECT_PREFIX", "")
	t.Setenv("FINSPO_SYNC_INTERVAL", "")
	t.Setenv("FINSPO_CAPTURE_PERMISSIONS", "")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load(): %v", err)
	}
	if cfg.SyncInterval.String() != "5m0s" {
		t.Fatalf("SyncInterval = %s, want 5m0s", cfg.SyncInterval)
	}
	if !cfg.CapturePermissions {
		t.Fatalf("CapturePermissions = false, want true")
	}

	if cfg.Port != "3130" {
		t.Fatalf("Port = %q, want 3130", cfg.Port)
	}
	if cfg.APIKeyHeader != "X-API-Key" {
		t.Fatalf("APIKeyHeader = %q, want X-API-Key", cfg.APIKeyHeader)
	}
	if cfg.APIKey != "test-key" {
		t.Fatalf("APIKey = %q, want test-key", cfg.APIKey)
	}
	if cfg.Environment != "dev" {
		t.Fatalf("Environment = %q, want dev", cfg.Environment)
	}
	if cfg.ServiceName != "finspo-core" {
		t.Fatalf("ServiceName = %q, want finspo-core", cfg.ServiceName)
	}
	if cfg.NATSSubjectPrefix != "finspo" {
		t.Fatalf("NATSSubjectPrefix = %q, want finspo", cfg.NATSSubjectPrefix)
	}
	if cfg.AllowExecution {
		t.Fatalf("AllowExecution = true, want false (destructive ops opt-in)")
	}
}

func TestLoadRespectsExplicitValues(t *testing.T) {
	setBaseEnv(t)
	t.Setenv("PORT", "4141")
	t.Setenv("FINSPO_API_KEY_HEADER", "X-Finspo-Key")
	t.Setenv("ENVIRONMENT", "prod")
	t.Setenv("NATS_URL", "nats://inter-plane-nats:4222")
	t.Setenv("NATS_SUBJECT_PREFIX", "finspo-prod")
	t.Setenv("OTEL_EXPORTER_OTLP_ENDPOINT", "http://otel:4318")
	t.Setenv("DATA_PLANE_DOCUMENTS_BASE_URL", "http://dpv2-documents-api:8010")
	t.Setenv("AUTH_CORE_URL", "http://auth-core:3011")
	t.Setenv("FINSPO_SERVICE_ID", "finspo-core-prod")
	t.Setenv("FINSPO_SERVICE_API_KEY", "finspo-service-key")
	t.Setenv("FINSPO_SYNC_INTERVAL", "30s")
	t.Setenv("FINSPO_CAPTURE_PERMISSIONS", "false")
	t.Setenv("FINSPO_ALLOW_EXECUTION", "true")
	t.Setenv("FINSPO_ARCHIVE_FOLDER_ID", "archive-123")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load(): %v", err)
	}

	if cfg.Port != "4141" {
		t.Fatalf("Port = %q, want 4141", cfg.Port)
	}
	if cfg.APIKeyHeader != "X-Finspo-Key" {
		t.Fatalf("APIKeyHeader = %q, want X-Finspo-Key", cfg.APIKeyHeader)
	}
	if cfg.Environment != "prod" {
		t.Fatalf("Environment = %q, want prod", cfg.Environment)
	}
	if cfg.NATSURL != "nats://inter-plane-nats:4222" {
		t.Fatalf("NATSURL = %q", cfg.NATSURL)
	}
	if cfg.NATSSubjectPrefix != "finspo-prod" {
		t.Fatalf("NATSSubjectPrefix = %q", cfg.NATSSubjectPrefix)
	}
	if cfg.OTELExporterEndpoint != "http://otel:4318" {
		t.Fatalf("OTELExporterEndpoint = %q", cfg.OTELExporterEndpoint)
	}
	if cfg.DataPlaneDocumentsURL != "http://dpv2-documents-api:8010" {
		t.Fatalf("DataPlaneDocumentsURL = %q", cfg.DataPlaneDocumentsURL)
	}
	if cfg.AuthCoreURL != "http://auth-core:3011" || cfg.FinspoServiceID != "finspo-core-prod" || cfg.FinspoServiceAPIKey != "finspo-service-key" {
		t.Fatalf("Data Plane service principal = %q/%q/%q", cfg.AuthCoreURL, cfg.FinspoServiceID, cfg.FinspoServiceAPIKey)
	}
	if cfg.SyncInterval.String() != "30s" {
		t.Fatalf("SyncInterval = %s, want 30s", cfg.SyncInterval)
	}
	if cfg.CapturePermissions {
		t.Fatalf("CapturePermissions = true, want false")
	}
	if !cfg.AllowExecution {
		t.Fatalf("AllowExecution = false, want true")
	}
	if cfg.ArchiveFolderID != "archive-123" {
		t.Fatalf("ArchiveFolderID = %q", cfg.ArchiveFolderID)
	}
}

func TestLoadFailsClosedWhenDataPlaneURLHasNoServicePrincipal(t *testing.T) {
	setBaseEnv(t)
	t.Setenv("DATA_PLANE_DOCUMENTS_BASE_URL", "http://dpv2-documents-api:8010")
	t.Setenv("AUTH_CORE_URL", "http://auth-core:3011")
	t.Setenv("FINSPO_SERVICE_API_KEY", "")

	if _, err := Load(); err == nil {
		t.Fatal("Load() accepted a Data Plane URL without FINSPO_SERVICE_API_KEY")
	}
}
