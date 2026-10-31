package config

import (
	"strings"
	"testing"
)

const syncDSNValue = "postgres://doffin_sync:sync-dsn-password@doffin-postgres:5432/anbud_sync?sslmode=disable"

// syncRequired is the smallest environment the sync needs. ANBUD_API_KEY,
// ANBUD_API_KEY_HEADER and PORT are absent on purpose: the sync serves no HTTP.
func syncRequired() map[string]string {
	return map[string]string{
		"ANBUD_DSN":      syncDSNValue,
		"DOFFIN_API_KEY": primaryKey,
	}
}

// readLog records every key a loader asks for, so a test can prove which
// variables were never read.
type readLog struct {
	vars  map[string]string
	reads map[string]bool
}

func newReadLog(vars map[string]string) *readLog {
	return &readLog{vars: vars, reads: make(map[string]bool)}
}

func (r *readLog) get(key string) string {
	r.reads[key] = true
	return r.vars[key]
}

func TestLoadSyncNeedsOnlyDSNAndPrimaryKey(t *testing.T) {
	t.Parallel()

	vars := with(syncRequired(), map[string]string{"PORT": "bogus"})
	s, err := LoadSyncFrom(env(vars))
	if err != nil {
		t.Fatalf("LoadSyncFrom: %v", err)
	}
	if s.DatabaseURL.Reveal() != syncDSNValue {
		t.Error("DatabaseURL was not read from ANBUD_DSN")
	}
	if s.Doffin.APIKey.Reveal() != primaryKey {
		t.Error("the primary Doffin key was not read from DOFFIN_API_KEY")
	}
	if s.Doffin.SecondaryAPIKey.IsSet() {
		t.Error("the sync loaded a secondary key; it must not")
	}
	if s.Doffin.SearchRPM != defaultDoffinSearchRPM || s.Doffin.DownloadRPM != defaultDoffinDownloadRPM {
		t.Errorf("RPM defaults = %d/%d, want %d/%d", s.Doffin.SearchRPM, s.Doffin.DownloadRPM, defaultDoffinSearchRPM, defaultDoffinDownloadRPM)
	}
	if s.Doffin.BaseURL != defaultDoffinBaseURL {
		t.Errorf("BaseURL = %q, want the production default", s.Doffin.BaseURL)
	}
	if s.ServiceName != "doffin-sync" {
		t.Errorf("ServiceName = %q, want doffin-sync", s.ServiceName)
	}

	named, err := LoadSyncFrom(env(with(vars, map[string]string{"SERVICE_NAME": "doffin-sync-nightly"})))
	if err != nil {
		t.Fatalf("LoadSyncFrom with SERVICE_NAME: %v", err)
	}
	if named.ServiceName != "doffin-sync-nightly" {
		t.Errorf("ServiceName = %q, want the SERVICE_NAME value", named.ServiceName)
	}
}

func TestLoadSyncNeverReadsSecondaryKey(t *testing.T) {
	t.Parallel()

	rl := newReadLog(with(syncRequired(), map[string]string{
		"DOFFIN_API_KEY_SECONDARY": secondaryKey,
		"ANBUD_API_KEY":            "internal-api-key-value",
		"ANBUD_API_KEY_HEADER":     "X-Custom-Key",
		"PORT":                     "3138",
	}))
	s, err := LoadSyncFrom(rl.get)
	if err != nil {
		t.Fatalf("LoadSyncFrom: %v", err)
	}
	for _, key := range []string{"DOFFIN_API_KEY_SECONDARY", "ANBUD_API_KEY", "ANBUD_API_KEY_HEADER", "PORT"} {
		if rl.reads[key] {
			t.Errorf("LoadSyncFrom read %s; the sync must never read it", key)
		}
	}
	if s.Doffin.SecondaryAPIKey.IsSet() {
		t.Error("the secondary key reached the sync configuration")
	}
}

func TestLoadSyncRequiresDoffinKey(t *testing.T) {
	t.Parallel()

	_, err := LoadSyncFrom(env(map[string]string{"ANBUD_DSN": syncDSNValue}))
	if err == nil {
		t.Fatal("expected an error when DOFFIN_API_KEY is missing")
	}
	if !strings.Contains(err.Error(), "DOFFIN_API_KEY is required") {
		t.Errorf("error should name DOFFIN_API_KEY: %v", err)
	}
}

func TestLoadSyncRequiresDSN(t *testing.T) {
	t.Parallel()

	_, err := LoadSyncFrom(env(map[string]string{"DOFFIN_API_KEY": primaryKey}))
	if err == nil {
		t.Fatal("expected an error when ANBUD_DSN is missing")
	}
	if !strings.Contains(err.Error(), "ANBUD_DSN is required") {
		t.Errorf("error should name ANBUD_DSN: %v", err)
	}
}

func TestLoadSyncErrorsNeverEchoValues(t *testing.T) {
	t.Parallel()

	const rpmMarker = "rpm-marker-7f3a"
	const dsnMarker = "dsn-marker-91c2"
	const baseMarker = "base-marker-44de"
	vars := map[string]string{
		"ANBUD_DSN":         "postgres://doffin_sync:" + dsnMarker + "@doffin-postgres:5432/anbud_sync",
		"DOFFIN_API_KEY":    primaryKey,
		"DOFFIN_SEARCH_RPM": "not-a-number-" + rpmMarker,
		"DOFFIN_BASE_URL":   "https://user:" + baseMarker + "@evil.example/v2",
	}
	_, err := LoadSyncFrom(env(vars))
	if err == nil {
		t.Fatal("expected an error")
	}
	if !strings.Contains(err.Error(), "DOFFIN_SEARCH_RPM") {
		t.Errorf("error should name DOFFIN_SEARCH_RPM: %v", err)
	}
	for _, leaked := range []string{rpmMarker, dsnMarker, baseMarker, primaryKey} {
		if strings.Contains(err.Error(), leaked) {
			t.Fatalf("error echoed a value: %q", err)
		}
	}
}
