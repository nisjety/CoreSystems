package config

import (
	"encoding/base64"
	"strings"
	"testing"
)

// The Integration Core switches added on this branch (W4-CC-3/4/7, W4-BYO, ZDR-Z-12-13) are dark by
// default and turn on only for the trimmed literal "true". strconv.ParseBool would also accept 1, t, T,
// TRUE and True, so a deployment that sets any of those must keep the old behaviour.

// strictSwitches maps each switch to the field it sets and the variables it needs to load when on.
var strictSwitches = []struct {
	env     string
	read    func(Config) bool
	prereqs map[string]string
}{
	{"CREDENTIAL_BINDINGS_ENABLED", func(c Config) bool { return c.CredentialBindingsEnabled }, nil},
	{"CUSTOMER_GATEWAY_CONNECTIONS_ENABLED", func(c Config) bool { return c.CustomerGatewayConnectionsEnabled },
		map[string]string{"CUSTOMER_GATEWAY_MODEL_PLANE_API_KEY": "mp-gateway-key-cccccccccccccccccccccccccc"}},
	{"CUSTOM_REST_CONNECTORS_ENABLED", func(c Config) bool { return c.CustomRESTConnectorsEnabled },
		map[string]string{"CREDENTIAL_BINDINGS_ENABLED": "true"}},
	{"ZDR_ACKNOWLEDGEMENT_ENABLED", func(c Config) bool { return c.ZDRAcknowledgementEnabled }, nil},
	{"INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED", func(c Config) bool { return c.ZDROrgEffectivePostureRead }, nil},
}

var allStrictSwitchEnvs = []string{
	"CREDENTIAL_BINDINGS_ENABLED", "CUSTOMER_GATEWAY_CONNECTIONS_ENABLED", "CUSTOM_REST_CONNECTORS_ENABLED",
	"ZDR_ACKNOWLEDGEMENT_ENABLED", "INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED", "CODEX_SUBSCRIPTION_ENABLED",
	"CUSTOMER_GATEWAY_MODEL_PLANE_API_KEY", "CREDENTIAL_REF_SEAL_KEYS", "ZDR_ACK_RETENTION_DAYS",
}

func clearStrictSwitchEnv(t *testing.T) {
	t.Helper()
	t.Setenv("INTEGRATION_CREDENTIALS_ENCRYPTION_KEY", base64.StdEncoding.EncodeToString([]byte("12345678901234567890123456789012")))
	for _, env := range allStrictSwitchEnvs {
		t.Setenv(env, "")
	}
}

func TestEveryBranchSwitchIsOffUnlessItIsTheLiteralTrue(t *testing.T) {
	for _, sw := range strictSwitches {
		t.Run(sw.env, func(t *testing.T) {
			for _, value := range []string{"", "1", "t", "T", "TRUE", "True", "yes", "on", "enabled", "false", "0", "truee", "\"true\""} {
				clearStrictSwitchEnv(t)
				for key, prereq := range sw.prereqs {
					t.Setenv(key, prereq)
				}
				t.Setenv(sw.env, value)
				cfg, err := Load()
				if err != nil {
					t.Fatalf("%s=%q: Load: %v", sw.env, value, err)
				}
				if sw.read(cfg) {
					t.Errorf("%s=%q turned the switch on; only the literal true may", sw.env, value)
				}
			}
			for _, value := range []string{"true", " true ", "true\n"} {
				clearStrictSwitchEnv(t)
				for key, prereq := range sw.prereqs {
					t.Setenv(key, prereq)
				}
				t.Setenv(sw.env, value)
				cfg, err := Load()
				if err != nil {
					t.Fatalf("%s=%q: Load: %v", sw.env, value, err)
				}
				if !sw.read(cfg) {
					t.Errorf("%s=%q left the switch off; the trimmed literal true turns it on", sw.env, value)
				}
			}
		})
	}
}

// With nothing set, every switch of this branch is off and the configuration is what it was before
// them: no broker, no bindings, no custom REST, no acknowledgement routes and the old posture read.
func TestWithNoSwitchSetTheBranchFeaturesAreAllOff(t *testing.T) {
	clearStrictSwitchEnv(t)
	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.CredentialBindingsEnabled || cfg.CustomerGatewayConnectionsEnabled || cfg.CustomRESTConnectorsEnabled ||
		cfg.ZDRAcknowledgementEnabled || cfg.ZDROrgEffectivePostureRead || cfg.CodexSubscriptionEnabled {
		t.Fatalf("a switch is on by default: %+v", map[string]bool{
			"CREDENTIAL_BINDINGS_ENABLED":               cfg.CredentialBindingsEnabled,
			"CUSTOMER_GATEWAY_CONNECTIONS_ENABLED":      cfg.CustomerGatewayConnectionsEnabled,
			"CUSTOM_REST_CONNECTORS_ENABLED":            cfg.CustomRESTConnectorsEnabled,
			"ZDR_ACKNOWLEDGEMENT_ENABLED":               cfg.ZDRAcknowledgementEnabled,
			"INTEGRATION_ZDR_EFFECTIVE_POSTURE_ENABLED": cfg.ZDROrgEffectivePostureRead,
			"CODEX_SUBSCRIPTION_ENABLED":                cfg.CodexSubscriptionEnabled,
		})
	}
	if cfg.CredentialRefSealKeys != "" || len(cfg.CredentialRefPrincipalKeys()) != 0 {
		t.Fatal("credential reference principals are configured by default")
	}
}

// runtimeConfig is a configuration that passes ValidateRuntime with every branch switch off: the
// subscription broker's valid runtime config with the broker turned off again.
func runtimeConfig(t *testing.T) Config {
	t.Helper()
	cfg := codexEnabledConfig(t)
	cfg.CodexSubscriptionEnabled = false
	cfg.InternalAPIKey = "internal-api-key-dddddddddddddddddddddddd"
	if err := cfg.ValidateRuntime(); err != nil {
		t.Fatalf("the base runtime config must validate: %v", err)
	}
	return cfg
}

func TestCustomRESTNeedsCredentialBindings(t *testing.T) {
	cfg := runtimeConfig(t)
	cfg.CustomRESTConnectorsEnabled = true
	if err := cfg.ValidateRuntime(); err == nil {
		t.Fatal("CUSTOM_REST_CONNECTORS_ENABLED=true validated without CREDENTIAL_BINDINGS_ENABLED=true")
	}
	cfg.CredentialBindingsEnabled = true
	if err := cfg.ValidateRuntime(); err != nil {
		t.Fatalf("custom REST with bindings on must validate: %v", err)
	}
}

func TestCredentialRefSealKeysMustBeDedicatedPerPrincipal(t *testing.T) {
	const gateway = "mg-seal-9d1c7a0b5e3f48a2b6c4d8e0f1a2b3c4"
	const execution = "ec-redeem-1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d"
	for name, value := range map[string]string{
		"an unknown principal":       "quarry-edge=" + gateway,
		"a repeated principal":       "model-gateway=" + gateway + ",model-gateway=" + execution,
		"a short key":                "model-gateway=short",
		"a placeholder key":          "model-gateway=placeholder-0123456789abcdef0123456789",
		"one key for both":           "model-gateway=" + gateway + ",execution-core=" + gateway,
		"an entry without a key":     "model-gateway",
		"the internal key reused":    "model-gateway=internal-key-0123456789abcdef0123456789",
		"an entry with an empty key": "execution-core=",
	} {
		t.Run(name, func(t *testing.T) {
			cfg := runtimeConfig(t)
			cfg.CredentialBindingsEnabled = true
			cfg.CredentialRefSealKeys = value
			if err := cfg.ValidateRuntime(); err == nil {
				t.Fatalf("CREDENTIAL_REF_SEAL_KEYS with %s validated", name)
			} else if strings.Contains(err.Error(), gateway) || strings.Contains(err.Error(), execution) {
				t.Fatalf("the validation error names a key value: %v", err)
			}
		})
	}
	cfg := runtimeConfig(t)
	cfg.CredentialBindingsEnabled = true
	cfg.CredentialRefSealKeys = " model-gateway = " + gateway + " , execution-core=" + execution
	if err := cfg.ValidateRuntime(); err != nil {
		t.Fatalf("two dedicated keys must validate: %v", err)
	}
	keys := cfg.CredentialRefPrincipalKeys()
	if keys[CredentialRefPrincipalModelGateway] != gateway || keys[CredentialRefPrincipalExecutionCore] != execution || len(keys) != 2 {
		t.Fatalf("principal keys %d entries, want exactly the two configured ones", len(keys))
	}
	// With the switch off, the keys are not checked: nothing reads them.
	off := runtimeConfig(t)
	off.CredentialRefSealKeys = "model-gateway=short"
	if err := off.ValidateRuntime(); err != nil {
		t.Fatalf("a bad key with the bindings switch off must not stop the service: %v", err)
	}
}

func TestTheAcknowledgementRetentionWindowIsClamped(t *testing.T) {
	for value, want := range map[string]int{"": 365, "1": 30, "29": 30, "30": 30, "400": 400, "3650": 3650, "99999": 3650, "-5": 365, "abc": 365} {
		clearStrictSwitchEnv(t)
		t.Setenv("ZDR_ACK_RETENTION_DAYS", value)
		cfg, err := Load()
		if err != nil {
			t.Fatalf("ZDR_ACK_RETENTION_DAYS=%q: %v", value, err)
		}
		if cfg.ZDRAcknowledgementRetentionDays != want {
			t.Errorf("ZDR_ACK_RETENTION_DAYS=%q gave %d days, want %d", value, cfg.ZDRAcknowledgementRetentionDays, want)
		}
	}
}
