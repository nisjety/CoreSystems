package config

import (
	"crypto/ed25519"
	"encoding/base64"
	"testing"
)

func conversationEnvironment(t *testing.T) {
	t.Helper()
	t.Setenv("DATABASE_URL", "postgres://test")
	t.Setenv("INTERNAL_API_KEY", "")
	t.Setenv("INTEGRATION_BASE_URL", "")
	t.Setenv("INTEGRATION_COREV2_URL", "")
	t.Setenv("INTEGRATION_INTERNAL_API_KEY", "")
	t.Setenv("AUTH_CORE_URL", "")
	t.Setenv("CONVERSATION_INTEGRATION_SERVICE_ID", "")
	t.Setenv("CONVERSATION_INTEGRATION_SERVICE_API_KEY", "")
	t.Setenv("CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY", "")
	t.Setenv("CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID", "")
	t.Setenv("CONVERSATION_GATEWAY_SERVICE_TOKEN", "gateway-test-secret-at-least-32-bytes")
	t.Setenv("CONVERSATION_CORE_INGEST_SERVICE_TOKEN", "ingest-test-secret-at-least-32-bytes-1")
}

func configureCompleteIntegration(t *testing.T) {
	t.Helper()
	t.Setenv("INTEGRATION_BASE_URL", "http://integration-api:3026")
	t.Setenv("INTEGRATION_INTERNAL_API_KEY", "explicit-integration-credential-32-bytes")
	t.Setenv("AUTH_CORE_URL", "http://auth-core:3011")
	t.Setenv("CONVERSATION_INTEGRATION_SERVICE_ID", "conversation-core")
	t.Setenv("CONVERSATION_INTEGRATION_SERVICE_API_KEY", "conversation-service-principal-credential-32-bytes")
	privateKey := ed25519.NewKeyFromSeed([]byte("0123456789abcdef0123456789abcdef"))
	t.Setenv("CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY", base64.StdEncoding.EncodeToString(privateKey))
	t.Setenv("CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID", "conversation-core-test-key")
}

func TestLoadDoesNotRequireLegacySharedKeyForConversationIngress(t *testing.T) {
	conversationEnvironment(t)
	if _, err := Load(); err != nil {
		t.Fatalf("Load() error = %v", err)
	}
}

func TestLoadRequiresCompleteIntegrationAuthorityWhenOutboundEnabled(t *testing.T) {
	conversationEnvironment(t)
	t.Setenv("INTEGRATION_BASE_URL", "http://integration-api:3026")
	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want incomplete integration authority rejection")
	}

	configureCompleteIntegration(t)
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() with complete integration authority error = %v", err)
	}
	if !cfg.DraftReplySendEnabled() {
		t.Fatal("DraftReplySendEnabled() = false with complete authority")
	}
}

func TestDraftReplySendEnabledRejectsNilAndIncompleteConfig(t *testing.T) {
	var nilConfig *Config
	if nilConfig.DraftReplySendEnabled() {
		t.Fatal("nil config enabled outbound sending")
	}
	if (&Config{IntegrationBaseURL: "http://integration-api:3026"}).DraftReplySendEnabled() {
		t.Fatal("partial config enabled outbound sending")
	}
}

func TestLoadRejectsEveryPartialIntegrationAuthorityConfiguration(t *testing.T) {
	values := map[string]string{
		"INTEGRATION_BASE_URL":                     "http://integration-api:3026",
		"INTEGRATION_INTERNAL_API_KEY":             "explicit-integration-credential-32-bytes",
		"AUTH_CORE_URL":                            "http://auth-core:3011",
		"CONVERSATION_INTEGRATION_SERVICE_ID":      "conversation-core",
		"CONVERSATION_INTEGRATION_SERVICE_API_KEY": "conversation-service-principal-credential-32-bytes",
	}
	for missing := range values {
		t.Run(missing, func(t *testing.T) {
			conversationEnvironment(t)
			for key, value := range values {
				if key != missing {
					t.Setenv(key, value)
				}
			}
			if _, err := Load(); err == nil {
				t.Fatalf("Load() error = nil, want missing %s rejection", missing)
			}
		})
	}
}

func TestLoadRejectsUnsafeIntegrationServicePrincipalConfiguration(t *testing.T) {
	tests := []struct {
		name, key, value string
	}{
		{name: "short credential", key: "CONVERSATION_INTEGRATION_SERVICE_API_KEY", value: "too-short"},
		{name: "placeholder credential", key: "CONVERSATION_INTEGRATION_SERVICE_API_KEY", value: "change-me-service-principal-credential-32-bytes"},
		{name: "underscore placeholder credential", key: "CONVERSATION_INTEGRATION_SERVICE_API_KEY", value: "CHANGE_ME_SERVICE_PRINCIPAL_CREDENTIAL_32_BYTES"},
		{name: "replace underscore credential", key: "CONVERSATION_INTEGRATION_SERVICE_API_KEY", value: "REPLACE_WITH_SERVICE_PRINCIPAL_CREDENTIAL_32_BYTES"},
		{name: "reused internal key", key: "CONVERSATION_INTEGRATION_SERVICE_API_KEY", value: "explicit-integration-credential-32-bytes"},
		{name: "reused gateway key", key: "CONVERSATION_INTEGRATION_SERVICE_API_KEY", value: "gateway-test-secret-at-least-32-bytes"},
		{name: "invalid service id", key: "CONVERSATION_INTEGRATION_SERVICE_ID", value: "conversation core"},
		{name: "userinfo auth URL", key: "AUTH_CORE_URL", value: "http://user:password@auth-core:3011"},
		{name: "query auth URL", key: "AUTH_CORE_URL", value: "http://auth-core:3011?credential=leak"},
		{name: "unsupported auth URL", key: "AUTH_CORE_URL", value: "file:///tmp/auth.sock"},
	}
	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			conversationEnvironment(t)
			configureCompleteIntegration(t)
			t.Setenv(testCase.key, testCase.value)
			if _, err := Load(); err == nil {
				t.Fatal("Load() error = nil, want unsafe service-principal configuration rejection")
			}
		})
	}
}

func TestLoadRequiresValidProviderWriteAttestationKeyWhenOutboundEnabled(t *testing.T) {
	for _, testCase := range []struct {
		name, key, value string
	}{
		{name: "missing private key", key: "CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY", value: ""},
		{name: "malformed private key", key: "CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY", value: "not-base64"},
		{name: "seed not private key", key: "CONVERSATION_PROVIDER_WRITE_ATTESTATION_PRIVATE_KEY", value: base64.StdEncoding.EncodeToString(make([]byte, ed25519.SeedSize))},
		{name: "missing key id", key: "CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID", value: ""},
		{name: "placeholder key id", key: "CONVERSATION_PROVIDER_WRITE_ATTESTATION_KEY_ID", value: "change-me-key"},
	} {
		t.Run(testCase.name, func(t *testing.T) {
			conversationEnvironment(t)
			configureCompleteIntegration(t)
			t.Setenv(testCase.key, testCase.value)
			if _, err := Load(); err == nil {
				t.Fatal("Load() error = nil")
			}
		})
	}
}

func TestLoadRejectsUnsafeOrReusedIntegrationCredential(t *testing.T) {
	for _, secret := range []string{
		"too-short",
		"change-me-integration-credential-32-bytes",
		"replace-with-integration-credential-32-bytes",
		"gateway-test-secret-at-least-32-bytes",
	} {
		t.Run(secret, func(t *testing.T) {
			conversationEnvironment(t)
			t.Setenv("INTEGRATION_BASE_URL", "http://integration-api:3026")
			t.Setenv("INTEGRATION_INTERNAL_API_KEY", secret)
			if _, err := Load(); err == nil {
				t.Fatal("Load() error = nil, want unsafe/reused integration credential rejection")
			}
		})
	}
}

func TestLoadRequiresConversationDelegationTokens(t *testing.T) {
	for _, missing := range []string{
		"CONVERSATION_GATEWAY_SERVICE_TOKEN",
		"CONVERSATION_CORE_INGEST_SERVICE_TOKEN",
	} {
		t.Run(missing, func(t *testing.T) {
			conversationEnvironment(t)
			t.Setenv(missing, "")
			if _, err := Load(); err == nil {
				t.Fatalf("Load() error = nil, want missing %s rejection", missing)
			}
		})
	}
}

func TestLoadRejectsShortAndPublishedPlaceholderDelegationTokens(t *testing.T) {
	for _, secret := range []string{
		"too-short",
		"replace-with-dedicated-random-32-byte-minimum-key",
		"change-me-conversation-gateway-secret-32-bytes",
	} {
		t.Run(secret, func(t *testing.T) {
			conversationEnvironment(t)
			t.Setenv("CONVERSATION_GATEWAY_SERVICE_TOKEN", secret)
			if _, err := Load(); err == nil {
				t.Fatal("Load() error = nil, want unsafe delegation token rejection")
			}
		})
	}
}

func TestLoadBuildsSeparateDelegationPrincipalKeys(t *testing.T) {
	conversationEnvironment(t)
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load() error = %v", err)
	}
	if cfg.DelegationKeys["verevon-gateway"] == "" || cfg.DelegationKeys["conversation-ingest"] == "" {
		t.Fatalf("DelegationKeys = %#v, want gateway and ingest principals", cfg.DelegationKeys)
	}
	if cfg.DelegationKeys["verevon-gateway"] == cfg.DelegationKeys["conversation-ingest"] {
		t.Fatal("gateway and ingest delegation tokens must be distinct")
	}
}

func TestLoadRejectsReusedDelegationToken(t *testing.T) {
	conversationEnvironment(t)
	t.Setenv("CONVERSATION_CORE_INGEST_SERVICE_TOKEN", "gateway-test-secret-at-least-32-bytes")
	if _, err := Load(); err == nil {
		t.Fatal("Load() error = nil, want reused delegation token rejection")
	}
}
