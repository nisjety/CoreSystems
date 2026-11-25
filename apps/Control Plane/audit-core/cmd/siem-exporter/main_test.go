package main

import (
	"strings"
	"testing"
)

func env(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

var enabledEnv = map[string]string{
	"AUDIT_SIEM_EXPORT_ENABLED": "true",
	"DATABASE_URL":              "postgres://u:p@db:5432/x",
	"ORG_CORE_URL":              "http://org-core:8080",
	"ORG_CORE_SERVICE_TOKEN":    "token",
}

func with(base map[string]string, extra map[string]string) map[string]string {
	out := map[string]string{}
	for k, v := range base {
		out[k] = v
	}
	for k, v := range extra {
		out[k] = v
	}
	return out
}

func TestTheExporterIsOffUnlessTheSwitchIsExactlyTrue(t *testing.T) {
	for _, raw := range []string{"", "0", "false", "off", "1", "TRUE", "True", "yes", "on", "enabled", " true", "true ", "2"} {
		cfg, err := loadConfig(env(map[string]string{"AUDIT_SIEM_EXPORT_ENABLED": raw}))
		if err != nil || cfg.Enabled {
			t.Errorf("AUDIT_SIEM_EXPORT_ENABLED=%q: enabled=%v err=%v, want off without error", raw, cfg.Enabled, err)
		}
	}
	cfg, err := loadConfig(env(enabledEnv))
	if err != nil || !cfg.Enabled {
		t.Fatalf("enabled env: %+v %v", cfg, err)
	}
}

func TestAnIdleExporterNeedsNoDatabaseOrOrgCoreSettings(t *testing.T) {
	cfg, err := loadConfig(env(map[string]string{}))
	if err != nil || cfg.Enabled || cfg.DatabaseURL != "" || cfg.OrgCoreToken != "" {
		t.Fatalf("idle config: %+v %v", cfg, err)
	}
}

func TestAnEnabledExporterRefusesToStartWithoutItsInputs(t *testing.T) {
	for _, missing := range []string{"DATABASE_URL", "ORG_CORE_URL", "ORG_CORE_SERVICE_TOKEN"} {
		values := with(enabledEnv, map[string]string{missing: ""})
		_, err := loadConfig(env(values))
		if err == nil || !strings.Contains(err.Error(), missing) {
			t.Errorf("without %s: %v", missing, err)
		}
		if err != nil && strings.Contains(err.Error(), "token") {
			t.Errorf("a config error must not echo a credential value: %v", err)
		}
	}
}

func TestTheDevOnlyPrivateDestinationSwitchIsRefusedInProduction(t *testing.T) {
	for name, values := range map[string]map[string]string{
		"idle":    {"AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS": "true", "ENVIRONMENT": "production"},
		"enabled": with(enabledEnv, map[string]string{"AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS": "true", "ENVIRONMENT": "production"}),
	} {
		_, err := loadConfig(env(values))
		if err == nil || err.Error() != "AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS=true is refused when ENVIRONMENT=production" {
			t.Errorf("%s: production accepted the dev switch: %v", name, err)
		}
	}
}

// A former spelling of either switch refuses boot in production, and reads as off
// everywhere else, with nothing turned on by the misspelt value.
func TestAFormerSpellingRefusesProductionBootAndReadsOffElsewhere(t *testing.T) {
	for _, name := range []string{"AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS", "AUDIT_SIEM_EXPORT_ENABLED"} {
		for _, raw := range []string{"1", "on", "yes", "TRUE", "True", "ture"} {
			_, err := loadConfig(env(map[string]string{name: raw, "ENVIRONMENT": "production"}))
			if err == nil || err.Error() != name+" must be true or false" {
				t.Errorf("%s=%q in production: err=%v, want the typed refusal", name, raw, err)
			}
			cfg, err := loadConfig(env(map[string]string{name: raw, "ENVIRONMENT": "development"}))
			if err != nil || cfg.Enabled || cfg.AllowPrivate {
				t.Errorf("%s=%q in development: %+v err=%v, want off without error", name, raw, cfg, err)
			}
		}
	}
}

func TestThePrivateDestinationSwitchIsStrictAndOnlyForDevelopment(t *testing.T) {
	dev := with(enabledEnv, map[string]string{"AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS": "true", "ENVIRONMENT": "development"})
	cfg, err := loadConfig(env(dev))
	if err != nil || !cfg.AllowPrivate {
		t.Fatalf("development with the switch: %+v %v", cfg, err)
	}
	for _, raw := range []string{"", "0", "false", "off"} {
		cfg, err := loadConfig(env(with(enabledEnv, map[string]string{"AUDIT_SIEM_ALLOW_PRIVATE_DESTINATIONS": raw, "ENVIRONMENT": "production"})))
		if err != nil || cfg.AllowPrivate {
			t.Errorf("switch %q in production: allow=%v err=%v, want off without error", raw, cfg.AllowPrivate, err)
		}
	}
}

func TestMetricsPortDefaultsAndOverrides(t *testing.T) {
	if cfg, _ := loadConfig(env(map[string]string{})); cfg.MetricsPort != 9091 {
		t.Errorf("default metrics port = %d", cfg.MetricsPort)
	}
	if cfg, _ := loadConfig(env(map[string]string{"METRICS_PORT": "9200"})); cfg.MetricsPort != 9200 {
		t.Errorf("override metrics port = %d", cfg.MetricsPort)
	}
	if cfg, _ := loadConfig(env(map[string]string{"METRICS_PORT": "nonsense"})); cfg.MetricsPort != 9091 {
		t.Errorf("invalid metrics port = %d", cfg.MetricsPort)
	}
}
