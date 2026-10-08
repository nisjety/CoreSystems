package config

import (
	"strings"
	"testing"
	"time"
)

func TestLoadUsesFailClosedControlTokenDefaults(t *testing.T) {
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "")
	t.Setenv("AUTH_CORE_ISSUER", "")
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_PUBLIC_KEY_FILE", "")

	cfg := Load()

	if cfg.JWTAudience != "data-plane" {
		t.Fatalf("JWT audience = %q", cfg.JWTAudience)
	}
	if cfg.JWTIssuer != "http://localhost:3011/api/convex-auth" {
		t.Fatalf("JWT issuer = %q", cfg.JWTIssuer)
	}
	if cfg.JWKSURL != "http://auth-core:3011/api/convex-auth/jwks" {
		t.Fatalf("JWKS URL = %q", cfg.JWKSURL)
	}
}

func TestLoadAcceptsExplicitVerificationConfiguration(t *testing.T) {
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "custom-data-plane")
	t.Setenv("AUTH_CORE_ISSUER", "https://auth.example/issuer")
	t.Setenv("AUTH_CORE_JWKS_URL", "https://auth.example/jwks")
	t.Setenv("JWT_PUBLIC_KEY_FILE", "/run/secrets/data-plane.pub")

	cfg := Load()

	if cfg.JWTAudience != "custom-data-plane" || cfg.JWTIssuer != "https://auth.example/issuer" {
		t.Fatalf("unexpected identity config: %+v", cfg)
	}
	if cfg.JWKSURL != "https://auth.example/jwks" || cfg.JWTPublicKeyFile != "/run/secrets/data-plane.pub" {
		t.Fatalf("unexpected key config: %+v", cfg)
	}
}

func TestLoadReadsDedicatedDataPlaneNATSToken(t *testing.T) {
	t.Setenv("DATAPLANE_NATS_TOKEN", "opaque-token")

	cfg := Load()
	if cfg.NatsToken != "opaque-token" {
		t.Fatalf("NatsToken was not loaded from the dedicated environment variable")
	}
}

func TestSignedCostConsumerConfigurationDefaultsOff(t *testing.T) {
	t.Setenv("SIGNED_COST_EVENTS_ENABLED", "")
	t.Setenv("EMBEDDING_EVENT_PUBLIC_KEY_PATH", "")
	t.Setenv("RETRIEVAL_EVENT_PUBLIC_KEY_PATH", "")
	t.Setenv("GRAPH_EVENT_PUBLIC_KEY_PATH", "")

	cfg := Load()
	if cfg.SignedCostEventsEnabled {
		t.Fatal("signed cost consumer unexpectedly enabled")
	}
}

// Every producer that emits usage has its own verification key, and a deployment missing one
// must not start: that producer's spend would be refused as invalid, silently, for ever.
func TestSignedCostConsumerConfigurationRequiresEveryProducerKey(t *testing.T) {
	keys := map[string]string{
		"EMBEDDING_EVENT_PUBLIC_KEY_PATH": "/run/event-keys/embedding.pub",
		"RETRIEVAL_EVENT_PUBLIC_KEY_PATH": "/run/event-keys/retrieval.pub",
		"GRAPH_EVENT_PUBLIC_KEY_PATH":     "/run/event-keys/graph.pub",
	}
	for missing := range keys {
		t.Setenv("SIGNED_COST_EVENTS_ENABLED", "1")
		for name, value := range keys {
			if name == missing {
				value = ""
			}
			t.Setenv(name, value)
		}
		if err := Load().ValidateSignedCostEvents(); err == nil {
			t.Fatalf("signed consumer accepted a registry without %s", missing)
		}
	}
	for name, value := range keys {
		t.Setenv(name, value)
	}
	if err := Load().ValidateSignedCostEvents(); err != nil {
		t.Fatalf("complete signed consumer configuration failed: %v", err)
	}
}

func setForwarder(t *testing.T, overrides map[string]string) *Config {
	t.Helper()
	values := map[string]string{
		"USAGE_FORWARDER_ENABLED":                "1",
		"COST_CORE_URL":                          "http://cost-core:8089",
		"COST_CORE_TOKEN_URL":                    "http://auth-core:3011/api/cost-core/internal-token",
		"DATA_ORCHESTRATOR_COST_SERVICE_ID":      "data-orchestrator",
		"DATA_ORCHESTRATOR_COST_SERVICE_API_KEY": strings.Repeat("k", 48),
		"USAGE_FORWARDER_BATCH":                  "",
		"USAGE_FORWARDER_INTERVAL_SECS":          "",
	}
	for name, value := range overrides {
		values[name] = value
	}
	for name, value := range values {
		t.Setenv(name, value)
	}
	return Load()
}

// The forwarder ships dark: nothing is posted to cost-core until an operator turns it on, after
// cost-core accepts the fields and prices the models.
func TestUsageForwarderIsOffByDefaultAndNeedsNothingWhileOff(t *testing.T) {
	for _, name := range []string{"USAGE_FORWARDER_ENABLED", "COST_CORE_URL", "COST_CORE_TOKEN_URL", "DATA_ORCHESTRATOR_COST_SERVICE_ID", "DATA_ORCHESTRATOR_COST_SERVICE_API_KEY", "USAGE_FORWARDER_BATCH", "USAGE_FORWARDER_INTERVAL_SECS"} {
		t.Setenv(name, "")
	}
	cfg := Load()
	if cfg.UsageForwarderEnabled {
		t.Fatal("the forwarder is on by default")
	}
	if err := cfg.ValidateUsageForwarder(); err != nil {
		t.Fatalf("a disabled forwarder demanded configuration: %v", err)
	}
	if cfg.CostCoreURL != "http://cost-core:8089" || cfg.CostCoreTokenURL != "http://auth-core:3011/api/cost-core/internal-token" ||
		cfg.CostServiceID != "data-orchestrator" || cfg.UsageForwarderBatch != 50 || cfg.UsageForwarderInterval != 15*time.Second {
		t.Fatalf("defaults = %+v", cfg)
	}
	// Anything but exactly "1" is off: a typo does not turn spend forwarding on.
	for _, value := range []string{"true", "yes", "on", "0", "2", " 1"} {
		if setForwarder(t, map[string]string{"USAGE_FORWARDER_ENABLED": value}).UsageForwarderEnabled {
			t.Fatalf("USAGE_FORWARDER_ENABLED=%q enabled the forwarder", value)
		}
	}
}

func TestEnabledUsageForwarderIsCheckedAtBoot(t *testing.T) {
	if err := setForwarder(t, nil).ValidateUsageForwarder(); err != nil {
		t.Fatalf("a complete configuration was refused: %v", err)
	}
	bad := map[string]map[string]string{
		"no key":                  {"DATA_ORCHESTRATOR_COST_SERVICE_API_KEY": ""},
		"a short key":             {"DATA_ORCHESTRATOR_COST_SERVICE_API_KEY": "short"},
		"a placeholder key":       {"DATA_ORCHESTRATOR_COST_SERVICE_API_KEY": "change-me-" + strings.Repeat("k", 40)},
		"no service id":           {"DATA_ORCHESTRATOR_COST_SERVICE_ID": "  "},
		"no cost-core address":    {"COST_CORE_URL": "not a url"},
		"a relative address":      {"COST_CORE_URL": "/api"},
		"an address with a login": {"COST_CORE_URL": "http://user:secret@cost-core:8089"},
		"an ftp address":          {"COST_CORE_URL": "ftp://cost-core"},
		"an address with a query": {"COST_CORE_URL": "http://cost-core:8089?x=1"},
		"a bad token address":     {"COST_CORE_TOKEN_URL": "auth-core"},
		"a zero batch":            {"USAGE_FORWARDER_BATCH": "0"},
		"a huge batch":            {"USAGE_FORWARDER_BATCH": "100000"},
		"a zero interval":         {"USAGE_FORWARDER_INTERVAL_SECS": "0"},
		"a day-long interval":     {"USAGE_FORWARDER_INTERVAL_SECS": "86400"},
	}
	for name, overrides := range bad {
		if err := setForwarder(t, overrides).ValidateUsageForwarder(); err == nil {
			t.Errorf("%s: an enabled forwarder with this configuration was accepted", name)
		}
	}
	// An unparsable number falls back to the default rather than to zero: a typo must not
	// make the forwarder spin.
	if cfg := setForwarder(t, map[string]string{"USAGE_FORWARDER_BATCH": "many"}); cfg.UsageForwarderBatch != 50 {
		t.Fatalf("a garbled batch became %d", cfg.UsageForwarderBatch)
	}
	// The credential is never in an error.
	key := strings.Repeat("z", 48)
	err := setForwarder(t, map[string]string{"DATA_ORCHESTRATOR_COST_SERVICE_API_KEY": key, "COST_CORE_URL": "nope"}).ValidateUsageForwarder()
	if err == nil || strings.Contains(err.Error(), key) {
		t.Fatalf("validation error = %v", err)
	}
}

func TestGDPRConsumerConfigurationDefaultsOff(t *testing.T) {
	t.Setenv("NATS_SHARED_URL", "")
	t.Setenv("NATS_SHARED_USER", "")
	t.Setenv("NATS_SHARED_PASSWORD", "")
	t.Setenv("GDPR_ORG_PURGE_CONSUMER_REQUIRED", "")

	cfg := Load()
	if err := cfg.ValidateGDPRConsumer(); err != nil {
		t.Fatalf("unconfigured GDPR consumer must not fail closed by default: %v", err)
	}
}

func TestGDPRConsumerConfigurationRequiresAllSharedCredentialsTogether(t *testing.T) {
	t.Setenv("NATS_SHARED_URL", "nats://control-shared-nats:4222")
	t.Setenv("NATS_SHARED_USER", "data-orchestrator-gdpr")
	t.Setenv("NATS_SHARED_PASSWORD", "")

	cfg := Load()
	if err := cfg.ValidateGDPRConsumer(); err == nil {
		t.Fatal("partial shared NATS credentials were accepted")
	}
}

func TestGDPRConsumerConfigurationRejectsWeakSharedPassword(t *testing.T) {
	t.Setenv("NATS_SHARED_URL", "nats://control-shared-nats:4222")
	t.Setenv("NATS_SHARED_USER", "data-orchestrator-gdpr")
	t.Setenv("NATS_SHARED_PASSWORD", "too-short")

	cfg := Load()
	if err := cfg.ValidateGDPRConsumer(); err == nil {
		t.Fatal("weak NATS_SHARED_PASSWORD was accepted")
	}
}

func TestGDPRConsumerConfigurationFailsClosedWhenRequiredButUnconfigured(t *testing.T) {
	t.Setenv("NATS_SHARED_URL", "")
	t.Setenv("NATS_SHARED_USER", "")
	t.Setenv("NATS_SHARED_PASSWORD", "")
	t.Setenv("GDPR_ORG_PURGE_CONSUMER_REQUIRED", "1")

	cfg := Load()
	if err := cfg.ValidateGDPRConsumer(); err == nil {
		t.Fatal("required GDPR org-purge consumer accepted missing shared NATS credentials")
	}
}

func TestGDPRConsumerConfigurationAcceptsCompleteSharedCredentials(t *testing.T) {
	t.Setenv("NATS_SHARED_URL", "nats://control-shared-nats:4222")
	t.Setenv("NATS_SHARED_USER", "data-orchestrator-gdpr")
	t.Setenv("NATS_SHARED_PASSWORD", strings.Repeat("a", 32))
	t.Setenv("GDPR_ORG_PURGE_CONSUMER_REQUIRED", "1")

	cfg := Load()
	if err := cfg.ValidateGDPRConsumer(); err != nil {
		t.Fatalf("complete shared NATS configuration failed: %v", err)
	}
}
