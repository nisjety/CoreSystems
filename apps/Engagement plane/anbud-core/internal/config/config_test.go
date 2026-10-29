package config

import (
	"fmt"
	"maps"
	"strings"
	"testing"
	"time"
)

func env(vars map[string]string) func(string) string {
	return func(key string) string { return vars[key] }
}

func required() map[string]string {
	return map[string]string{
		"ANBUD_API_KEY": "internal-api-key-value",
		"ANBUD_DSN":     "postgres://ingestion_user:db-password-value@ingestion-postgres:5432/anbud_core?sslmode=disable",
	}
}

func with(base map[string]string, extra map[string]string) map[string]string {
	out := make(map[string]string, len(base)+len(extra))
	maps.Copy(out, base)
	maps.Copy(out, extra)
	return out
}

func TestLoadFromAppliesDefaults(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(required()))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}

	if cfg.Port != "3138" {
		t.Errorf("Port = %q, want 3138", cfg.Port)
	}
	if cfg.Environment != "dev" || cfg.ServiceName != "anbud-core" {
		t.Errorf("Environment/ServiceName = %q/%q", cfg.Environment, cfg.ServiceName)
	}
	if cfg.APIKeyHeader != "X-API-Key" {
		t.Errorf("APIKeyHeader = %q", cfg.APIKeyHeader)
	}
	if cfg.Doffin.BaseURL != "https://api.doffin.no/public/v2" {
		t.Errorf("Doffin.BaseURL = %q", cfg.Doffin.BaseURL)
	}
	// Defaults keep ~20% headroom under the documented 30/min and 10/min per key.
	if cfg.Doffin.SearchRPM != 24 || cfg.Doffin.DownloadRPM != 8 {
		t.Errorf("Doffin RPM = %d/%d, want 24/8", cfg.Doffin.SearchRPM, cfg.Doffin.DownloadRPM)
	}
	if cfg.Doffin.HTTPTimeout != 30*time.Second {
		t.Errorf("Doffin.HTTPTimeout = %v", cfg.Doffin.HTTPTimeout)
	}
	if cfg.Doffin.Configured() {
		t.Error("Doffin must be unconfigured when DOFFIN_API_KEY is absent")
	}
}

func TestLoadFromReportsEveryMissingRequiredVariable(t *testing.T) {
	t.Parallel()

	_, err := LoadFrom(env(map[string]string{}))
	if err == nil {
		t.Fatal("expected an error when required variables are missing")
	}
	for _, name := range []string{"ANBUD_API_KEY", "ANBUD_DSN"} {
		if !strings.Contains(err.Error(), name) {
			t.Errorf("error %q should name %s", err, name)
		}
	}
}

func TestLoadFromAllowsDoffinToBeOptional(t *testing.T) {
	t.Parallel()

	// The service must boot without the Doffin key so the plane can come up
	// before the operator has pasted it in; it then reports "not configured".
	cfg, err := LoadFrom(env(required()))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if cfg.Doffin.Configured() {
		t.Fatal("expected Doffin to be unconfigured")
	}
}

func TestLoadFromTrimsPastedKeys(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(with(required(), map[string]string{
		"DOFFIN_API_KEY": "  0123456789abcdef0123456789abcdef \n",
	})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if !cfg.Doffin.Configured() {
		t.Fatal("expected Doffin to be configured")
	}
	if got := cfg.Doffin.APIKey.Reveal(); got != "0123456789abcdef0123456789abcdef" {
		t.Fatalf("key not trimmed: %q", got)
	}
}

func TestLoadFromRejectsInvalidValues(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		vars     map[string]string
		wantName string
	}{
		{"port not numeric", map[string]string{"PORT": "abc"}, "PORT"},
		{"port too large", map[string]string{"PORT": "70000"}, "PORT"},
		{"port zero", map[string]string{"PORT": "0"}, "PORT"},
		{"search rpm not numeric", map[string]string{"DOFFIN_SEARCH_RPM": "fast"}, "DOFFIN_SEARCH_RPM"},
		{"search rpm zero", map[string]string{"DOFFIN_SEARCH_RPM": "0"}, "DOFFIN_SEARCH_RPM"},
		{"search rpm negative", map[string]string{"DOFFIN_SEARCH_RPM": "-3"}, "DOFFIN_SEARCH_RPM"},
		{"search rpm over the documented limit", map[string]string{"DOFFIN_SEARCH_RPM": "31"}, "DOFFIN_SEARCH_RPM"},
		{"download rpm over the documented limit", map[string]string{"DOFFIN_DOWNLOAD_RPM": "11"}, "DOFFIN_DOWNLOAD_RPM"},
		{"download rpm not numeric", map[string]string{"DOFFIN_DOWNLOAD_RPM": "x"}, "DOFFIN_DOWNLOAD_RPM"},
		{"timeout not a duration", map[string]string{"DOFFIN_HTTP_TIMEOUT": "soon"}, "DOFFIN_HTTP_TIMEOUT"},
		{"timeout not positive", map[string]string{"DOFFIN_HTTP_TIMEOUT": "0s"}, "DOFFIN_HTTP_TIMEOUT"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			_, err := LoadFrom(env(with(required(), tc.vars)))
			if err == nil {
				t.Fatal("expected an error")
			}
			if !strings.Contains(err.Error(), tc.wantName) {
				t.Fatalf("error %q should name %s", err, tc.wantName)
			}
		})
	}
}

func TestLoadFromErrorsNeverEchoSecretValues(t *testing.T) {
	t.Parallel()

	_, err := LoadFrom(env(with(required(), map[string]string{
		"DOFFIN_API_KEY":    "0123456789abcdef0123456789abcdef",
		"DOFFIN_SEARCH_RPM": "nope",
	})))
	if err == nil {
		t.Fatal("expected an error")
	}
	for _, secretValue := range []string{"0123456789abcdef0123456789abcdef", "internal-api-key-value", "db-password-value"} {
		if strings.Contains(err.Error(), secretValue) {
			t.Fatalf("error leaked a secret: %q", err)
		}
	}
}

func TestConfigNeverRendersSecrets(t *testing.T) {
	t.Parallel()

	cfg, err := LoadFrom(env(with(required(), map[string]string{
		"DOFFIN_API_KEY": "0123456789abcdef0123456789abcdef",
	})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}

	rendered := fmt.Sprintf("%v | %+v | %#v", cfg, cfg, cfg)
	for _, secretValue := range []string{"0123456789abcdef0123456789abcdef", "internal-api-key-value", "db-password-value"} {
		if strings.Contains(rendered, secretValue) {
			t.Fatalf("config rendering leaked %q in %s", secretValue, rendered)
		}
	}
}

func TestLoadDoffinFromDoesNotRequireServiceVariables(t *testing.T) {
	t.Parallel()

	// cmd/doffin-probe only needs the Doffin settings, not the DSN or API key.
	d, err := LoadDoffinFrom(env(map[string]string{"DOFFIN_API_KEY": "0123456789abcdef0123456789abcdef"}))
	if err != nil {
		t.Fatalf("LoadDoffinFrom: %v", err)
	}
	if !d.Configured() {
		t.Fatal("expected configured")
	}
}

func TestLoadReadsTheProcessEnvironment(t *testing.T) {
	t.Setenv("ANBUD_API_KEY", "internal-api-key-value")
	t.Setenv("ANBUD_DSN", "postgres://u:p@h:5432/anbud_core")

	if _, err := Load(); err != nil {
		t.Fatalf("Load: %v", err)
	}
}
