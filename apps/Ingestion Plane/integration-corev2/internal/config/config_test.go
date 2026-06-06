package config

import (
	"encoding/base64"
	"strings"
	"testing"
)

func TestLoadRequiresEncryptionKey(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", "")
	_, err := Load()
	if err == nil || !strings.Contains(err.Error(), "INTEGRATION_CREDENTIALS_ENCRYPTION_KEY") {
		t.Fatalf("Load error = %v, want encryption key error", err)
	}
}

func TestLoadAcceptsBase64EncryptionKey(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("INTERNAL_API_KEY", "dev")
	t.Setenv("DATABASE_URL", "postgres://example")
	t.Setenv("INTEGRATION_WEBHOOK_HOTPATH_URL", "http://integration-webhook-normalizer:3036/")
	t.Setenv("SCIM_ORG_BEARER_TOKENS", "org-1:token-1, org-2:token-2")
	t.Setenv("INTEGRATION_TOKEN_LEASE_CONSUMERS", "finspo-core, data-plane-v2, finspo-core")
	t.Setenv("FINSPO_API_KEY", "finspo-key")
	t.Setenv("FINSPO_API_KEY_HEADER", "X-Finspo-Key")
	t.Setenv("DATA_PLANE_INTERNAL_API_KEY", "data-plane-key")
	t.Setenv("DATA_PLANE_INTERNAL_API_KEY_HEADER", "X-Data-Plane-Key")
	t.Setenv("INTEGRATION_CORE_URL", "http://integration-corev2:3026/")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}
	if len(cfg.EncryptionKey) != 32 {
		t.Fatalf("EncryptionKey length = %d, want 32", len(cfg.EncryptionKey))
	}
	if cfg.Port != "3026" {
		t.Fatalf("Port = %q, want 3026", cfg.Port)
	}
	if cfg.WebhookHotPathURL != "http://integration-webhook-normalizer:3036" {
		t.Fatalf("WebhookHotPathURL = %q, want trimmed hot path URL", cfg.WebhookHotPathURL)
	}
	if cfg.SCIMBearerTokens["org-1"] != "token-1" || cfg.SCIMBearerTokens["org-2"] != "token-2" {
		t.Fatalf("SCIMBearerTokens = %#v, want parsed org token map", cfg.SCIMBearerTokens)
	}
	if len(cfg.TokenLeaseConsumers) != 2 || cfg.TokenLeaseConsumers[0] != "finspo-core" || cfg.TokenLeaseConsumers[1] != "data-plane-v2" {
		t.Fatalf("TokenLeaseConsumers = %#v, want deduped consumers", cfg.TokenLeaseConsumers)
	}
	if cfg.FinspoCoreAPIKey != "finspo-key" || cfg.FinspoCoreAPIKeyHeader != "X-Finspo-Key" {
		t.Fatalf("Finspo auth = %q/%q, want configured key/header", cfg.FinspoCoreAPIKey, cfg.FinspoCoreAPIKeyHeader)
	}
	if cfg.DataPlaneInternalAPIKey != "data-plane-key" || cfg.DataPlaneInternalAPIHeader != "X-Data-Plane-Key" {
		t.Fatalf("Data Plane auth = %q/%q, want configured key/header", cfg.DataPlaneInternalAPIKey, cfg.DataPlaneInternalAPIHeader)
	}
	if cfg.IntegrationCoreURL != "http://integration-corev2:3026" {
		t.Fatalf("IntegrationCoreURL = %q, want trimmed internal URL", cfg.IntegrationCoreURL)
	}
}

func TestValidateFinspoWorkerRuntime(t *testing.T) {
	cfg := Config{
		InternalAPIKey:     "internal",
		IntegrationCoreURL: "http://integration-corev2:3026",
		FinspoCoreURL:      "http://finspo-api:3130",
		FinspoCoreAPIKey:   "finspo-key",
	}
	if err := cfg.ValidateFinspoWorkerRuntime(); err != nil {
		t.Fatalf("ValidateFinspoWorkerRuntime returned error: %v", err)
	}

	cfg.FinspoCoreAPIKey = ""
	err := cfg.ValidateFinspoWorkerRuntime()
	if err == nil || !strings.Contains(err.Error(), "FINSPO_API_KEY") {
		t.Fatalf("ValidateFinspoWorkerRuntime error = %v, want FINSPO_API_KEY", err)
	}
}

func TestLoadAcceptsLegacyMicrosoftCredentialEnv(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("AZURE_CLIENT_ID", "")
	t.Setenv("AZURE_CLIENT_SECRET", "")
	t.Setenv("MICROSOFT_CLIENT_ID", "legacy-client-id")
	t.Setenv("MICROSOFT_CLIENT_SECRET", "legacy-client-secret")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}
	if cfg.MicrosoftClientID != "legacy-client-id" {
		t.Fatalf("MicrosoftClientID = %q, want legacy-client-id", cfg.MicrosoftClientID)
	}
	if cfg.MicrosoftClientSecret != "legacy-client-secret" {
		t.Fatalf("MicrosoftClientSecret = %q, want legacy-client-secret", cfg.MicrosoftClientSecret)
	}
}

func TestControlPlaneInternalAPIKeyFallsBackToInternalAPIKey(t *testing.T) {
	cfg := Config{InternalAPIKey: "shared-key"}
	if got := cfg.ControlPlaneInternalAPIKey(); got != "shared-key" {
		t.Fatalf("ControlPlaneInternalAPIKey = %q, want shared-key", got)
	}

	cfg.AuthCoreInternalAPIKey = "auth-key"
	if got := cfg.ControlPlaneInternalAPIKey(); got != "auth-key" {
		t.Fatalf("ControlPlaneInternalAPIKey = %q, want auth-key", got)
	}
}

func TestValidateProviderRequiresMicrosoftCredentials(t *testing.T) {
	cfg := Config{}
	err := cfg.ValidateProvider("microsoft")
	if err == nil || !strings.Contains(err.Error(), "AZURE_CLIENT_ID") {
		t.Fatalf("ValidateProvider error = %v, want client id error", err)
	}
}

func TestValidateProviderRequiresProviderSpecificCredentials(t *testing.T) {
	tests := []struct {
		provider string
		want     string
	}{
		{provider: "slack", want: "SLACK_CLIENT_ID"},
		{provider: "google", want: "GOOGLE_CLIENT_ID"},
		{provider: "notion", want: "NOTION_CLIENT_ID"},
		{provider: "github", want: "GITHUB_CLIENT_ID"},
		{provider: "shopify", want: "SHOPIFY_CLIENT_ID"},
	}
	for _, tt := range tests {
		t.Run(tt.provider, func(t *testing.T) {
			err := Config{}.ValidateProvider(tt.provider)
			if err == nil || !strings.Contains(err.Error(), tt.want) {
				t.Fatalf("ValidateProvider error = %v, want %s", err, tt.want)
			}
		})
	}
}
