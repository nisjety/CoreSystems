package config

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
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

func TestLoadAndValidateRuntimeRequireTrustedProviderWriteAttestationKeys(t *testing.T) {
	publicKey, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	rawKeys, err := json.Marshal([]map[string]string{{
		"issuer":     "conversation-core",
		"kid":        "conversation-write-2026-07",
		"public_key": base64.StdEncoding.EncodeToString(publicKey),
	}})
	if err != nil {
		t.Fatalf("Marshal trusted keys error: %v", err)
	}
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON", string(rawKeys))
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load error: %v", err)
	}
	if cfg.ProviderWriteAttestationKeysJSON != string(rawKeys) {
		t.Fatal("Load did not preserve trusted public-key JSON")
	}
	cfg.InternalAPIKey = "internal"
	cfg.AuthCoreURL = "http://auth-core:3011"
	cfg.AuthCoreInternalAPIKey = "auth-core-key"
	cfg.OrgCoreURL = "http://org-core:8080"
	cfg.OrgCoreServiceToken = "org-service-token-at-least-32-bytes"
	cfg.BillingCoreServiceToken = "billing-service-token-at-least-32-bytes"
	cfg.AuditCoreServiceToken = "audit-service-token-at-least-32-bytes"
	cfg.PublicBaseURL = "http://integration-corev2:3026"
	cfg.DatabaseURL = "postgres://integration"
	if err := cfg.ValidateRuntime(); err != nil {
		t.Fatalf("ValidateRuntime error: %v", err)
	}

	for _, invalid := range []string{"", "[]", `[{"issuer":"conversation-core","kid":"change-me","public_key":"invalid"}]`} {
		cfg.ProviderWriteAttestationKeysJSON = invalid
		if err := cfg.ValidateRuntime(); err == nil || !strings.Contains(err.Error(), "INTEGRATION_PROVIDER_WRITE_ATTESTATION_KEYS_JSON") {
			t.Fatalf("ValidateRuntime error = %v for unsafe keys, want provider-write key error", err)
		}
	}
}

func TestValidateRuntimeFailsClosedForEveryRequiredBoundary(t *testing.T) {
	publicKey, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("GenerateKey error: %v", err)
	}
	rawKeys, err := json.Marshal([]map[string]string{{
		"issuer": "conversation-core", "kid": "conversation-write-2026-07", "public_key": base64.StdEncoding.EncodeToString(publicKey),
	}})
	if err != nil {
		t.Fatalf("Marshal keys error: %v", err)
	}
	valid := Config{
		InternalAPIKey: "internal", AuthCoreURL: "http://auth-core:3011", AuthCoreInternalAPIKey: "auth-key",
		OrgCoreURL: "http://org-core:8080", PublicBaseURL: "http://integration-corev2:3026",
		OrgCoreServiceToken: "org-service-token-at-least-32-bytes", BillingCoreServiceToken: "billing-service-token-at-least-32-bytes",
		AuditCoreServiceToken: "audit-service-token-at-least-32-bytes",
		DatabaseURL:           "postgres://integration", EncryptionKey: []byte("12345678901234567890123456789012"),
		ProviderWriteAttestationKeysJSON: string(rawKeys),
	}
	tests := []struct {
		name   string
		update func(*Config)
		want   string
	}{
		{name: "internal key", update: func(c *Config) { c.InternalAPIKey = "" }, want: "INTERNAL_API_KEY"},
		{name: "auth url", update: func(c *Config) { c.AuthCoreURL = "" }, want: "AUTH_CORE_URL"},
		{name: "control key", update: func(c *Config) { c.AuthCoreInternalAPIKey = ""; c.InternalAPIKey = "" }, want: "INTERNAL_API_KEY"},
		{name: "org url", update: func(c *Config) { c.OrgCoreURL = "" }, want: "ORG_CORE_URL"},
		{name: "org token", update: func(c *Config) { c.OrgCoreServiceToken = "" }, want: "ORG_CORE_SERVICE_TOKEN"},
		{name: "test org token", update: func(c *Config) { c.OrgCoreServiceToken = "test-org-service-token-at-least-32-bytes" }, want: "ORG_CORE_SERVICE_TOKEN"},
		{name: "placeholder billing token", update: func(c *Config) { c.BillingCoreServiceToken = "placeholder-billing-service-token-at-least-32-bytes" }, want: "BILLING_CORE_SERVICE_TOKEN"},
		{name: "change-me billing token", update: func(c *Config) { c.BillingCoreServiceToken = "change-me-billing-service-token-at-least-32-bytes" }, want: "BILLING_CORE_SERVICE_TOKEN"},
		{name: "replace-with audit token", update: func(c *Config) { c.AuditCoreServiceToken = "replace-with-audit-service-token-at-least-32-bytes" }, want: "AUDIT_CORE_SERVICE_TOKEN"},
		{name: "short audit token", update: func(c *Config) { c.AuditCoreServiceToken = "short" }, want: "AUDIT_CORE_SERVICE_TOKEN"},
		{name: "duplicate audience token", update: func(c *Config) { c.AuditCoreServiceToken = c.OrgCoreServiceToken }, want: "reuse"},
		{name: "reuse internal key", update: func(c *Config) { c.InternalAPIKey = c.OrgCoreServiceToken }, want: "reuse"},
		{name: "reuse auth internal key", update: func(c *Config) { c.AuthCoreInternalAPIKey = c.BillingCoreServiceToken }, want: "reuse"},
		{name: "public url", update: func(c *Config) { c.PublicBaseURL = "" }, want: "INTEGRATION_PUBLIC_BASE_URL"},
		{name: "database", update: func(c *Config) { c.DatabaseURL = "" }, want: "DATABASE_URL"},
		{name: "encryption", update: func(c *Config) { c.EncryptionKey = []byte("short") }, want: "INTEGRATION_CREDENTIALS_ENCRYPTION_KEY"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			cfg := valid
			tt.update(&cfg)
			if err := cfg.ValidateRuntime(); err == nil || !strings.Contains(err.Error(), tt.want) {
				t.Fatalf("ValidateRuntime error = %v, want %s", err, tt.want)
			}
		})
	}
	allowMemory := valid
	allowMemory.DatabaseURL = ""
	allowMemory.AllowInMemoryStore = true
	if err := allowMemory.ValidateRuntime(); err != nil {
		t.Fatalf("ValidateRuntime in-memory opt-in error: %v", err)
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
	t.Setenv("INTEGRATION_SERVICE_ID", "integration-corev2-prod")
	t.Setenv("INTEGRATION_SERVICE_API_KEY", "integration-service-key")
	t.Setenv("INTEGRATION_CORE_URL", "http://integration-corev2:3026/")
	t.Setenv("FACEBOOK_CLIENT_ID", "facebook-app-id")
	t.Setenv("META_APP_CLIENT_ID", "meta-app-id")
	t.Setenv("META_APP_CLIENT_SECRET", "meta-app-secret")
	t.Setenv("META_JS_SDK_API_VERSION", "23.0")
	t.Setenv("META_JS_SDK_LOCALE", "nb_NO")
	t.Setenv("META_BUSINESS_LOGIN_CONFIG_ID", "business-login-config")
	t.Setenv("META_BUSINESS_LOGIN_SUPPORT_MESSAGING_CONFIG_ID", "support-messaging-config")
	t.Setenv("META_BUSINESS_LOGIN_STUDIO_ADS_CONFIG_ID", "studio-ads-config")
	t.Setenv("META_WEBHOOK_VERIFY_TOKEN", "meta-verify-token")
	t.Setenv("META_WEBHOOK_SECRET", "meta-webhook-secret")
	t.Setenv("THREADS_API_BASE_URL", "https://threads.test/v1/")

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
	if cfg.IntegrationServiceID != "integration-corev2-prod" || cfg.IntegrationServiceAPIKey != "integration-service-key" {
		t.Fatalf("Data Plane service principal = %q/%q", cfg.IntegrationServiceID, cfg.IntegrationServiceAPIKey)
	}
	if cfg.IntegrationCoreURL != "http://integration-corev2:3026" {
		t.Fatalf("IntegrationCoreURL = %q, want trimmed internal URL", cfg.IntegrationCoreURL)
	}
	if cfg.MicrosoftTokenOrigin != "http://localhost:3026" {
		t.Fatalf("MicrosoftTokenOrigin = %q, want public base origin", cfg.MicrosoftTokenOrigin)
	}
	if cfg.LinkedInMarketingVersion != "202606" {
		t.Fatalf("LinkedInMarketingVersion = %q, want default 202606", cfg.LinkedInMarketingVersion)
	}
	if cfg.MetaClientID != "meta-app-id" || cfg.MetaClientSecret != "meta-app-secret" {
		t.Fatalf("Meta OAuth credentials = %q/%q, want meta-app-id/meta-app-secret", cfg.MetaClientID, cfg.MetaClientSecret)
	}
	if cfg.MetaJSSDKAppID != "meta-app-id" || cfg.MetaJSSDKAPIVersion != "v23.0" || cfg.MetaJSSDKLocale != "nb_NO" {
		t.Fatalf("Meta JS SDK config = %q/%q/%q, want meta-app-id/v23.0/nb_NO", cfg.MetaJSSDKAppID, cfg.MetaJSSDKAPIVersion, cfg.MetaJSSDKLocale)
	}
	if cfg.MetaBusinessLoginConfigID != "business-login-config" {
		t.Fatalf("MetaBusinessLoginConfigID = %q, want configured id", cfg.MetaBusinessLoginConfigID)
	}
	if cfg.MetaBusinessLoginConfigIDs["studio_ads"] != "studio-ads-config" {
		t.Fatalf("MetaBusinessLoginConfigIDs[studio_ads] = %q, want configured studio ads id", cfg.MetaBusinessLoginConfigIDs["studio_ads"])
	}
	if cfg.MetaBusinessLoginConfigIDs["support_messaging"] != "support-messaging-config" {
		t.Fatalf("MetaBusinessLoginConfigIDs[support_messaging] = %q, want configured support messaging id", cfg.MetaBusinessLoginConfigIDs["support_messaging"])
	}
	if cfg.MetaWebhookVerifyToken != "meta-verify-token" || cfg.MetaWebhookSecret != "meta-webhook-secret" || cfg.MetaThreadsAPIBaseURL != "https://threads.test/v1" {
		t.Fatalf("Meta webhook/Threads config = %q/%q/%q, want configured values", cfg.MetaWebhookVerifyToken, cfg.MetaWebhookSecret, cfg.MetaThreadsAPIBaseURL)
	}
	if cfg.InstagramClientID != "" {
		t.Fatalf("InstagramClientID = %q, want empty when a dedicated Instagram client is not configured", cfg.InstagramClientID)
	}
}

func TestNormalizeGraphAPIVersionDefaultsToCurrentVersion(t *testing.T) {
	if got := normalizeGraphAPIVersion(""); got != "v25.0" {
		t.Fatalf("normalizeGraphAPIVersion(\"\") = %q, want v25.0", got)
	}
	if got := normalizeGraphAPIVersion("25.0"); got != "v25.0" {
		t.Fatalf("normalizeGraphAPIVersion(\"25.0\") = %q, want v25.0", got)
	}
}

func TestNormalizeGitHubAPIBaseURLAvoidsPublicWebOrigin404(t *testing.T) {
	if got := normalizeGitHubAPIBaseURL("https://github.com"); got != "https://api.github.com" {
		t.Fatalf("normalizeGitHubAPIBaseURL(public web) = %q, want https://api.github.com", got)
	}
	if got := normalizeGitHubAPIBaseURL("https://github.example.com"); got != "https://github.example.com/api/v3" {
		t.Fatalf("normalizeGitHubAPIBaseURL(enterprise web) = %q, want enterprise API path", got)
	}
	if got := normalizeGitHubAPIBaseURL("https://github.example.com/api/v3/"); got != "https://github.example.com/api/v3" {
		t.Fatalf("normalizeGitHubAPIBaseURL(enterprise API) = %q, want trimmed enterprise API path", got)
	}
	if got := normalizeGitHubAPIBaseURL("http://localhost:9090"); got != "http://localhost:9090" {
		t.Fatalf("normalizeGitHubAPIBaseURL(local mock) = %q, want unchanged local mock URL", got)
	}
}

func TestLoadAcceptsTikTokClientIDAlias(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("TIKTOK_CLIENT_KEY", "")
	t.Setenv("TIKTOK_CLIENT_ID", "legacy-tiktok-client-id")
	t.Setenv("TIKTOK_CLIENT_SECRET", "tiktok-secret")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}
	if cfg.TikTokClientKey != "legacy-tiktok-client-id" {
		t.Fatalf("TikTokClientKey = %q, want client_id alias", cfg.TikTokClientKey)
	}
	if missing := cfg.ProviderReadiness()["tiktok"]; len(missing) != 0 {
		t.Fatalf("TikTok readiness missing = %v, want ready", missing)
	}
}

func TestSnapchatReadinessRequiresHTTPSRedirectBaseURL(t *testing.T) {
	cfg := Config{
		PublicBaseURL:        "http://localhost:3026",
		SnapchatClientID:     "snapchat-client",
		SnapchatClientSecret: "snapchat-secret",
	}
	missing := strings.Join(cfg.ProviderReadiness()["snapchat"], ",")
	if !strings.Contains(missing, "SNAPCHAT_REDIRECT_BASE_URL or HTTPS INTEGRATION_PUBLIC_BASE_URL") {
		t.Fatalf("Snapchat readiness missing = %q, want HTTPS redirect requirement", missing)
	}

	cfg.SnapchatRedirectBaseURL = "https://connect.example.com/"
	if missing := cfg.ProviderReadiness()["snapchat"]; len(missing) != 0 {
		t.Fatalf("Snapchat readiness missing = %v, want ready with HTTPS redirect override", missing)
	}
	if err := cfg.ValidateProvider("snapchat"); err != nil {
		t.Fatalf("ValidateProvider returned error with HTTPS redirect override: %v", err)
	}
}

func TestLoadTrimsSnapchatRedirectBaseURL(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("SNAPCHAT_REDIRECT_BASE_URL", "https://connect.example.com/")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load returned error: %v", err)
	}
	if cfg.SnapchatRedirectBaseURL != "https://connect.example.com" {
		t.Fatalf("SnapchatRedirectBaseURL = %q, want trimmed override", cfg.SnapchatRedirectBaseURL)
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

	cfg.FinspoCoreAPIKey = "finspo-key"
	cfg.DataPlaneDocumentsURL = "http://dpv2-documents-api:8010"
	cfg.AuthCoreURL = "http://auth-core:3011"
	cfg.IntegrationServiceID = "integration-corev2"
	cfg.IntegrationServiceAPIKey = ""
	err = cfg.ValidateFinspoWorkerRuntime()
	if err == nil || !strings.Contains(err.Error(), "INTEGRATION_SERVICE_API_KEY") {
		t.Fatalf("ValidateFinspoWorkerRuntime error = %v, want INTEGRATION_SERVICE_API_KEY", err)
	}
}

func TestValidateEmailWorkerRequiresDedicatedConversationIngestToken(t *testing.T) {
	cfg := Config{
		DatabaseURL:                    "postgres://test",
		EncryptionKey:                  []byte("12345678901234567890123456789012"),
		ConversationIngestURL:          "http://conversation-ingest-rs:3161",
		ConversationIngestServiceToken: "integration-email-worker-test-secret-at-least-32-bytes",
		InternalAPIKey:                 "internal",
		IntegrationCoreURL:             "http://integration-corev2:3026",
	}
	if err := cfg.ValidateEmailWorkerRuntime(); err != nil {
		t.Fatalf("ValidateEmailWorkerRuntime() error = %v", err)
	}

	for _, unsafe := range []string{"", "too-short", "change-me-email-worker-token-at-least-32-bytes"} {
		cfg.ConversationIngestServiceToken = unsafe
		if err := cfg.ValidateEmailWorkerRuntime(); err == nil {
			t.Fatalf("ValidateEmailWorkerRuntime() error = nil for unsafe token %q", unsafe)
		}
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
	if cfg.MicrosoftClientAuthMode != "public" {
		t.Fatalf("MicrosoftClientAuthMode = %q, want public", cfg.MicrosoftClientAuthMode)
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

func TestValidateProviderAllowsMicrosoftPublicClientWithoutSecret(t *testing.T) {
	cfg := Config{
		MicrosoftClientID:       "microsoft-client",
		MicrosoftClientAuthMode: "public",
	}
	if err := cfg.ValidateProvider("microsoft"); err != nil {
		t.Fatalf("ValidateProvider returned error: %v", err)
	}
}

func TestValidateProviderRequiresMicrosoftSecretForConfidentialClient(t *testing.T) {
	cfg := Config{
		MicrosoftClientID:       "microsoft-client",
		MicrosoftClientAuthMode: "confidential",
	}
	err := cfg.ValidateProvider("microsoft")
	if err == nil || !strings.Contains(err.Error(), "AZURE_CLIENT_SECRET") {
		t.Fatalf("ValidateProvider error = %v, want client secret error", err)
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
		{provider: "linkedin", want: "LINKEDIN_CLIENT_ID"},
		{provider: "x", want: "X_CLIENT_ID"},
		{provider: "instagram", want: "INSTAGRAM_CLIENT_ID"},
		{provider: "meta", want: "FACEBOOK_CLIENT_ID"},
		{provider: "facebook", want: "FACEBOOK_CLIENT_ID"},
		{provider: "whatsapp", want: "FACEBOOK_CLIENT_ID"},
		{provider: "meta-ads", want: "FACEBOOK_CLIENT_ID"},
		{provider: "snapchat", want: "SNAPCHAT_CLIENT_ID"},
		{provider: "tiktok", want: "TIKTOK_CLIENT_KEY or TIKTOK_CLIENT_ID"},
		{provider: "discord", want: "DISCORD_CLIENT_ID"},
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

func TestLoadReadsOptionalDiscordBotToken(t *testing.T) {
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	t.Setenv("DISCORD_BOT_TOKEN", "  bot-token-value  ")
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load error: %v", err)
	}
	if cfg.DiscordBotToken != "bot-token-value" {
		t.Fatalf("DiscordBotToken = %q, want trimmed token", cfg.DiscordBotToken)
	}

	// The Discord inbox source is optional: the email worker runtime must
	// validate without DISCORD_BOT_TOKEN.
	workerCfg := Config{
		DatabaseURL:                    "postgres://test",
		EncryptionKey:                  []byte("12345678901234567890123456789012"),
		ConversationIngestURL:          "http://conversation-ingest-rs:3161",
		ConversationIngestServiceToken: "integration-email-worker-test-secret-at-least-32-bytes",
		InternalAPIKey:                 "internal",
		IntegrationCoreURL:             "http://integration-corev2:3026",
	}
	if err := workerCfg.ValidateEmailWorkerRuntime(); err != nil {
		t.Fatalf("ValidateEmailWorkerRuntime() error = %v, want DISCORD_BOT_TOKEN to stay optional", err)
	}
}
