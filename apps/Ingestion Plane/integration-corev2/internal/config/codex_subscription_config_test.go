package config

import (
	"crypto/ed25519"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"slices"
	"strings"
	"testing"
	"time"
)

// codexEnabledConfig is a valid runtime config with the subscription broker
// on and every broker knob at its default.
func codexEnabledConfig(t *testing.T) Config {
	t.Helper()
	publicKey, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	rawKeys, _ := json.Marshal([]map[string]string{{"issuer": "conversation-core", "kid": "conversation-write-2026-07", "public_key": base64.StdEncoding.EncodeToString(publicKey)}})
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
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
	cfg.ProviderWriteAttestationKeysJSON = string(rawKeys)
	cfg.CodexSubscriptionEnabled = true
	cfg.CodexSubscriptionHome = "/var/lib/integration-core/codex-subscriptions"
	cfg.CodexSubscriptionModelPlaneAPIKey = "codex-model-plane-key"
	return cfg
}

func TestCodexSubscriptionKnobDefaults(t *testing.T) {
	cfg := codexEnabledConfig(t)
	if err := cfg.ValidateRuntime(); err != nil {
		t.Fatalf("defaults must validate: %v", err)
	}
	if cfg.CodexSubscriptionTransport != "app-server" ||
		cfg.CodexSubscriptionDirectBaseURL != "https://chatgpt.com/backend-api/codex" ||
		cfg.CodexSubscriptionDirectOriginator != "verevon" ||
		cfg.CodexSubscriptionDirectRequestShape != "classic" ||
		cfg.CodexSubscriptionDirectPromptMode != "roles" ||
		cfg.CodexSubscriptionDirectMaxRetries != 2 ||
		cfg.CodexSubscriptionRefreshSkew != 10*time.Minute ||
		cfg.CodexSubscriptionMaxConcurrency != 16 ||
		cfg.CodexSubscriptionMaxConcurrencyPerConnection != 4 ||
		cfg.CodexSubscriptionQueueTimeout != 30*time.Second ||
		!cfg.CodexAppServerHardened ||
		!cfg.CodexAppServerThreadEffort {
		t.Fatalf("unexpected defaults: %+v", cfg)
	}
	if missing := cfg.ProviderReadiness()["openai-codex-subscription"]; len(missing) != 0 {
		t.Fatalf("readiness = %v, want none missing", missing)
	}
	if len(cfg.CodexSubscriptionWarnings()) != 0 {
		t.Fatalf("defaults warn: %v", cfg.CodexSubscriptionWarnings())
	}
}

func TestTransportSelection(t *testing.T) {
	for raw, want := range map[string]string{"app-server": "app-server", "APP-SERVER": "app-server", "direct": "direct"} {
		t.Setenv("CODEX_SUBSCRIPTION_TRANSPORT", raw)
		cfg := codexEnabledConfig(t)
		if cfg.CodexSubscriptionTransport != want {
			t.Fatalf("%q loaded as %q", raw, cfg.CodexSubscriptionTransport)
		}
		if err := cfg.ValidateRuntime(); err != nil {
			t.Fatalf("%q: %v", raw, err)
		}
	}
	t.Setenv("CODEX_SUBSCRIPTION_TRANSPORT", "grpc")
	cfg := codexEnabledConfig(t)
	if err := cfg.ValidateRuntime(); err == nil || !strings.Contains(err.Error(), "CODEX_SUBSCRIPTION_TRANSPORT") {
		t.Fatalf("grpc error = %v", err)
	}
	if !slices.Contains(cfg.ProviderReadiness()["openai-codex-subscription"], "CODEX_SUBSCRIPTION_TRANSPORT") {
		t.Fatal("readiness does not name the invalid transport")
	}
	// A disabled broker does not validate its knobs.
	cfg.CodexSubscriptionEnabled = false
	if err := cfg.ValidateRuntime(); err != nil {
		t.Fatalf("disabled broker: %v", err)
	}
}

func TestDirectConfigValidation(t *testing.T) {
	cases := []struct {
		name   string
		update func(*Config)
		env    string
	}{
		{"http base url", func(c *Config) { c.CodexSubscriptionDirectBaseURL = "http://chatgpt.com/backend-api/codex" }, "CODEX_SUBSCRIPTION_DIRECT_BASE_URL"},
		{"other host", func(c *Config) { c.CodexSubscriptionDirectBaseURL = "https://api.openai.com/v1" }, "CODEX_SUBSCRIPTION_DIRECT_BASE_URL"},
		{"lookalike host", func(c *Config) { c.CodexSubscriptionDirectBaseURL = "https://chatgpt.com.evil.test/backend-api/codex" }, "CODEX_SUBSCRIPTION_DIRECT_BASE_URL"},
		{"port", func(c *Config) { c.CodexSubscriptionDirectBaseURL = "https://chatgpt.com:8443/backend-api/codex" }, "CODEX_SUBSCRIPTION_DIRECT_BASE_URL"},
		{"userinfo", func(c *Config) { c.CodexSubscriptionDirectBaseURL = "https://user:pass@chatgpt.com/backend-api/codex" }, "CODEX_SUBSCRIPTION_DIRECT_BASE_URL"},
		{"query", func(c *Config) { c.CodexSubscriptionDirectBaseURL = "https://chatgpt.com/backend-api/codex?x=1" }, "CODEX_SUBSCRIPTION_DIRECT_BASE_URL"},
		{"fragment", func(c *Config) { c.CodexSubscriptionDirectBaseURL = "https://chatgpt.com/backend-api/codex#x" }, "CODEX_SUBSCRIPTION_DIRECT_BASE_URL"},
		{"codex_cli_rs", func(c *Config) { c.CodexSubscriptionDirectOriginator = "codex_cli_rs" }, "CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR"},
		{"codex-tui", func(c *Config) { c.CodexSubscriptionDirectOriginator = "codex-tui" }, "CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR"},
		{"any codex", func(c *Config) { c.CodexSubscriptionDirectOriginator = "codexish" }, "CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR"},
		{"opencode", func(c *Config) { c.CodexSubscriptionDirectOriginator = "opencode" }, "CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR"},
		{"pi", func(c *Config) { c.CodexSubscriptionDirectOriginator = "pi" }, "CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR"},
		{"bad characters", func(c *Config) { c.CodexSubscriptionDirectOriginator = "Verevon!" }, "CODEX_SUBSCRIPTION_DIRECT_ORIGINATOR"},
		{"shape", func(c *Config) { c.CodexSubscriptionDirectRequestShape = "huge" }, "CODEX_SUBSCRIPTION_DIRECT_REQUEST_SHAPE"},
		{"prompt mode", func(c *Config) { c.CodexSubscriptionDirectPromptMode = "chatml" }, "CODEX_SUBSCRIPTION_DIRECT_PROMPT_MODE"},
		{"retries high", func(c *Config) { c.CodexSubscriptionDirectMaxRetries = 4 }, "CODEX_SUBSCRIPTION_DIRECT_MAX_RETRIES"},
		{"retries negative", func(c *Config) { c.CodexSubscriptionDirectMaxRetries = -1 }, "CODEX_SUBSCRIPTION_DIRECT_MAX_RETRIES"},
		{"skew low", func(c *Config) { c.CodexSubscriptionRefreshSkew = 30 * time.Second }, "CODEX_SUBSCRIPTION_REFRESH_SKEW"},
		{"skew high", func(c *Config) { c.CodexSubscriptionRefreshSkew = 25 * time.Hour }, "CODEX_SUBSCRIPTION_REFRESH_SKEW"},
		{"concurrency", func(c *Config) { c.CodexSubscriptionMaxConcurrency = 0 }, "CODEX_SUBSCRIPTION_MAX_CONCURRENCY"},
		{"per connection above global", func(c *Config) { c.CodexSubscriptionMaxConcurrencyPerConnection = 17 }, "CODEX_SUBSCRIPTION_MAX_CONCURRENCY_PER_CONNECTION"},
		{"per connection zero", func(c *Config) { c.CodexSubscriptionMaxConcurrencyPerConnection = 0 }, "CODEX_SUBSCRIPTION_MAX_CONCURRENCY_PER_CONNECTION"},
		{"queue zero", func(c *Config) { c.CodexSubscriptionQueueTimeout = 0 }, "CODEX_SUBSCRIPTION_QUEUE_TIMEOUT"},
		{"queue above invocation", func(c *Config) { c.CodexSubscriptionQueueTimeout = 2 * time.Minute }, "CODEX_SUBSCRIPTION_QUEUE_TIMEOUT"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			cfg := codexEnabledConfig(t)
			tc.update(&cfg)
			if err := cfg.ValidateRuntime(); err == nil || !strings.Contains(err.Error(), tc.env) {
				t.Fatalf("ValidateRuntime error = %v, want %s", err, tc.env)
			}
			if !slices.Contains(cfg.ProviderReadiness()["openai-codex-subscription"], tc.env) {
				t.Fatalf("readiness does not name %s", tc.env)
			}
		})
	}
	valid := codexEnabledConfig(t)
	valid.CodexSubscriptionDirectMaxRetries = 0
	valid.CodexSubscriptionDirectOriginator = "verevon-staging"
	valid.CodexSubscriptionRefreshSkew = time.Minute
	if err := valid.ValidateRuntime(); err != nil {
		t.Fatalf("zero retries and a 1m skew are valid: %v", err)
	}
	if warnings := valid.CodexSubscriptionWarnings(); len(warnings) != 1 || !strings.Contains(warnings[0], "CODEX_SUBSCRIPTION_REFRESH_SKEW") {
		t.Fatalf("a 1m skew must warn: %v", warnings)
	}
}

func TestCodexSubscriptionUnparseableKnobsFailStartup(t *testing.T) {
	for env, value := range map[string]string{
		"CODEX_SUBSCRIPTION_DIRECT_MAX_RETRIES":             "two",
		"CODEX_SUBSCRIPTION_REFRESH_SKEW":                   "ten minutes",
		"CODEX_SUBSCRIPTION_MAX_CONCURRENCY":                "lots",
		"CODEX_SUBSCRIPTION_MAX_CONCURRENCY_PER_CONNECTION": "4.5",
		"CODEX_SUBSCRIPTION_QUEUE_TIMEOUT":                  "30",
		"CODEX_APP_SERVER_HARDENED":                         "maybe",
		"CODEX_APP_SERVER_THREAD_EFFORT":                    "sometimes",
	} {
		t.Run(env, func(t *testing.T) {
			t.Setenv(env, value)
			cfg := codexEnabledConfig(t)
			if err := cfg.ValidateRuntime(); err == nil || !strings.Contains(err.Error(), env) {
				t.Fatalf("ValidateRuntime error = %v, want %s", err, env)
			}
		})
	}
	t.Setenv("CODEX_SUBSCRIPTION_DIRECT_MAX_RETRIES", "0")
	t.Setenv("CODEX_APP_SERVER_HARDENED", "false")
	cfg := codexEnabledConfig(t)
	if cfg.CodexSubscriptionDirectMaxRetries != 0 || cfg.CodexAppServerHardened {
		t.Fatalf("explicit 0 retries / hardened=false were not kept: %d %v", cfg.CodexSubscriptionDirectMaxRetries, cfg.CodexAppServerHardened)
	}
	if err := cfg.ValidateRuntime(); err != nil {
		t.Fatal(err)
	}
}

// CODEX_APP_SERVER_THREAD_EFFORT is the no-rebuild rollback for the
// thread/start effort override: unset means on, and every strconv.ParseBool
// spelling is honoured.
func TestCodexAppServerThreadEffortParsing(t *testing.T) {
	for value, want := range map[string]bool{
		"": true, "   ": true,
		"true": true, "TRUE": true, "True": true, "t": true, "1": true, " true ": true,
		"false": false, "FALSE": false, "False": false, "f": false, "0": false, " false ": false,
	} {
		t.Run(value, func(t *testing.T) {
			t.Setenv("CODEX_APP_SERVER_THREAD_EFFORT", value)
			cfg := codexEnabledConfig(t)
			if cfg.CodexAppServerThreadEffort != want {
				t.Fatalf("CODEX_APP_SERVER_THREAD_EFFORT=%q loaded as %v, want %v", value, cfg.CodexAppServerThreadEffort, want)
			}
			if err := cfg.ValidateRuntime(); err != nil {
				t.Fatalf("CODEX_APP_SERVER_THREAD_EFFORT=%q: %v", value, err)
			}
		})
	}
	t.Setenv("CODEX_APP_SERVER_THREAD_EFFORT", "off")
	cfg := codexEnabledConfig(t)
	if !cfg.CodexAppServerThreadEffort {
		t.Fatal("an unparseable value must keep the default while startup rejects it")
	}
	if !slices.Contains(cfg.ProviderReadiness()["openai-codex-subscription"], "CODEX_APP_SERVER_THREAD_EFFORT") {
		t.Fatal("readiness does not name the unparseable CODEX_APP_SERVER_THREAD_EFFORT")
	}
}
