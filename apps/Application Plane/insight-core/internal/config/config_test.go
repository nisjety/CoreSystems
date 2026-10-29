package config

import "testing"

func TestLoadRequiresInternalAPIKey(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "")

	_, err := Load()
	if err == nil {
		t.Fatal("Load error = nil, want missing INTERNAL_API_KEY error")
	}
}

func TestLoadUsesSafeConnectorDefaults(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "test-key")
	t.Setenv("PORT", "not-a-port")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load error: %v", err)
	}
	if cfg.HTTPPort != 3163 {
		t.Fatalf("HTTPPort = %d, want default 3163", cfg.HTTPPort)
	}
	if cfg.ServiceName != "insight-core" {
		t.Fatalf("ServiceName = %s, want insight-core", cfg.ServiceName)
	}
	if cfg.ConnectorTokenLeaseAudience != "insight-core" {
		t.Fatalf("ConnectorTokenLeaseAudience = %s, want insight-core", cfg.ConnectorTokenLeaseAudience)
	}
	if cfg.GoogleAnalyticsAPIBaseURL != "https://analyticsdata.googleapis.com" {
		t.Fatalf("GoogleAnalyticsAPIBaseURL = %s", cfg.GoogleAnalyticsAPIBaseURL)
	}
	if cfg.GoogleSearchConsoleAPIBaseURL != "https://www.googleapis.com/webmasters/v3" {
		t.Fatalf("GoogleSearchConsoleAPIBaseURL = %s", cfg.GoogleSearchConsoleAPIBaseURL)
	}
}

func TestLoadHonorsConnectorOverrides(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "test-key")
	t.Setenv("PORT", "3210")
	t.Setenv("SERVICE_NAME", "custom-insight-core")
	t.Setenv("INTEGRATION_CORE_URL", "http://integration.local/")
	t.Setenv("INSIGHT_CONNECTOR_TOKEN_LEASE_AUDIENCE", "custom-audience")
	t.Setenv("GOOGLE_ANALYTICS_DATA_API_BASE_URL", "https://analytics.example.test/")
	t.Setenv("GOOGLE_SEARCH_CONSOLE_API_BASE_URL", "https://search.example.test/")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load error: %v", err)
	}
	if cfg.HTTPPort != 3210 {
		t.Fatalf("HTTPPort = %d, want 3210", cfg.HTTPPort)
	}
	if cfg.ServiceName != "custom-insight-core" {
		t.Fatalf("ServiceName = %s", cfg.ServiceName)
	}
	if cfg.IntegrationCoreURL != "http://integration.local" {
		t.Fatalf("IntegrationCoreURL = %s", cfg.IntegrationCoreURL)
	}
	if cfg.ConnectorTokenLeaseAudience != "custom-audience" {
		t.Fatalf("ConnectorTokenLeaseAudience = %s", cfg.ConnectorTokenLeaseAudience)
	}
	if cfg.GoogleAnalyticsAPIBaseURL != "https://analytics.example.test" {
		t.Fatalf("GoogleAnalyticsAPIBaseURL = %s", cfg.GoogleAnalyticsAPIBaseURL)
	}
	if cfg.GoogleSearchConsoleAPIBaseURL != "https://search.example.test" {
		t.Fatalf("GoogleSearchConsoleAPIBaseURL = %s", cfg.GoogleSearchConsoleAPIBaseURL)
	}
}

func TestLoadRejectsModelPlaneTokenFallbackAndRequiresScopedCredentialPair(t *testing.T) {
	t.Setenv("INTERNAL_API_KEY", "test-key")
	t.Setenv("MODEL_PLANE_NATS_URL", "nats://model-nats:4222")
	t.Setenv("MODEL_PLANE_NATS_USER", "")
	t.Setenv("MODEL_PLANE_NATS_PASSWORD", "")
	t.Setenv("MODEL_PLANE_NATS_TOKEN", "legacy-token-must-not-work")

	if _, err := Load(); err == nil {
		t.Fatal("Model Plane URL without scoped user/password was accepted")
	}

	t.Setenv("MODEL_PLANE_NATS_USER", "application-insight-model")
	t.Setenv("MODEL_PLANE_NATS_PASSWORD", "0123456789abcdef0123456789abcdef")
	cfg, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if cfg.ModelPlaneNATSUser != "application-insight-model" || cfg.ModelPlaneNATSPassword == "" {
		t.Fatalf("unexpected scoped Model credential config: %+v", cfg)
	}
}
