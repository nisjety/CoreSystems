package config

import (
	"fmt"
	"strings"
	"testing"
)

// ANBUD_MIGRATE_DSN is the optional second DSN: when it is set, migrations run
// through it and the runtime pool uses ANBUD_DSN (O2, least privilege). These
// tests are pure. They read the environment through a map and need no database.

const (
	migrateDSNValue    = "postgres://anbud_migrator:migrate-dsn-password@doffin-postgres:5432/anbud_core?sslmode=disable"
	migrateDSNPassword = "migrate-dsn-password"
)

func TestLoadSyncMigrateDSNIsOptional(t *testing.T) {
	t.Parallel()

	s, err := LoadSyncFrom(env(syncRequired()))
	if err != nil {
		t.Fatalf("LoadSyncFrom without ANBUD_MIGRATE_DSN: %v", err)
	}
	if s.MigrateDatabaseURL.IsSet() {
		t.Error("MigrateDatabaseURL is set although ANBUD_MIGRATE_DSN is not")
	}
	if s.DatabaseURL.Reveal() != syncDSNValue {
		t.Error("DatabaseURL changed: with one DSN the sync must behave exactly as before")
	}
}

func TestLoadSyncReadsMigrateDSN(t *testing.T) {
	t.Parallel()

	s, err := LoadSyncFrom(env(with(syncRequired(), map[string]string{"ANBUD_MIGRATE_DSN": migrateDSNValue})))
	if err != nil {
		t.Fatalf("LoadSyncFrom: %v", err)
	}
	if got := s.MigrateDatabaseURL.Reveal(); got != migrateDSNValue {
		t.Error("MigrateDatabaseURL was not read from ANBUD_MIGRATE_DSN")
	}
	if got := s.DatabaseURL.Reveal(); got != syncDSNValue {
		t.Error("DatabaseURL must still come from ANBUD_DSN alone")
	}
}

func TestLoadSyncMigrateDSNIsTrimmedAndBlankMeansUnset(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name  string
		value string
		set   bool
	}{
		{"surrounding space is trimmed", "  " + migrateDSNValue + "\n", true},
		{"only space counts as unset", "   \t", false},
		{"empty counts as unset", "", false},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			t.Parallel()

			s, err := LoadSyncFrom(env(with(syncRequired(), map[string]string{"ANBUD_MIGRATE_DSN": c.value})))
			if err != nil {
				t.Fatalf("LoadSyncFrom: %v", err)
			}
			if s.MigrateDatabaseURL.IsSet() != c.set {
				t.Fatalf("IsSet = %v, want %v", s.MigrateDatabaseURL.IsSet(), c.set)
			}
			if c.set && s.MigrateDatabaseURL.Reveal() != migrateDSNValue {
				t.Error("the value was not trimmed")
			}
		})
	}
}

// The service configuration carries it too, so cmd/api can adopt the same split.
func TestLoadFromReadsMigrateDSN(t *testing.T) {
	t.Parallel()

	unset, err := LoadFrom(env(required()))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if unset.MigrateDatabaseURL.IsSet() {
		t.Error("MigrateDatabaseURL is set although ANBUD_MIGRATE_DSN is not")
	}

	set, err := LoadFrom(env(with(required(), map[string]string{"ANBUD_MIGRATE_DSN": migrateDSNValue})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if set.MigrateDatabaseURL.Reveal() != migrateDSNValue {
		t.Error("MigrateDatabaseURL was not read from ANBUD_MIGRATE_DSN")
	}
	if set.DatabaseURL.Reveal() != required()["ANBUD_DSN"] {
		t.Error("DatabaseURL must still come from ANBUD_DSN alone")
	}
}

// A DSN carries the database password, so neither configuration may render it.
func TestMigrateDSNIsNeverRendered(t *testing.T) {
	t.Parallel()

	s, err := LoadSyncFrom(env(with(syncRequired(), map[string]string{"ANBUD_MIGRATE_DSN": migrateDSNValue})))
	if err != nil {
		t.Fatalf("LoadSyncFrom: %v", err)
	}
	c, err := LoadFrom(env(with(required(), map[string]string{"ANBUD_MIGRATE_DSN": migrateDSNValue})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	for _, shown := range []string{
		fmt.Sprintf("%v", s), fmt.Sprintf("%+v", s), fmt.Sprintf("%#v", s),
		fmt.Sprintf("%v", c), fmt.Sprintf("%+v", c), fmt.Sprintf("%#v", c),
	} {
		for _, leak := range []string{migrateDSNValue, migrateDSNPassword, "anbud_migrator"} {
			if strings.Contains(shown, leak) {
				t.Fatalf("a printed configuration carries part of ANBUD_MIGRATE_DSN (%q)", leak)
			}
		}
	}
}

// A problem elsewhere in the environment must not make the loader print the
// migration DSN: errors name variables and never echo a value.
func TestLoadErrorsNeverEchoMigrateDSN(t *testing.T) {
	t.Parallel()

	vars := map[string]string{
		"ANBUD_DSN":         syncDSNValue,
		"ANBUD_MIGRATE_DSN": migrateDSNValue,
		"DOFFIN_SEARCH_RPM": "not-a-number",
	}
	for name, load := range map[string]func() error{
		"sync":    func() error { _, err := LoadSyncFrom(env(vars)); return err },
		"service": func() error { _, err := LoadFrom(env(vars)); return err },
	} {
		err := load()
		if err == nil {
			t.Fatalf("%s: expected an error", name)
		}
		for _, leak := range []string{migrateDSNValue, migrateDSNPassword, "anbud_migrator"} {
			if strings.Contains(err.Error(), leak) {
				t.Fatalf("%s: the error echoes part of ANBUD_MIGRATE_DSN (%q): %v", name, leak, err)
			}
		}
	}
}

// The variable's name is declared once, here, and exported: cmd/doffin-sync names it in its messages
// and in the TLS rule through config.EnvMigrateDSN and has no copy of its own. Both loaders read
// the variable by that name, so a test that sets it through the constant reaches both.
func TestEnvMigrateDSNIsTheOneNameBothLoadersRead(t *testing.T) {
	t.Parallel()

	if EnvMigrateDSN != "ANBUD_MIGRATE_DSN" {
		t.Fatalf("EnvMigrateDSN = %q, want ANBUD_MIGRATE_DSN", EnvMigrateDSN)
	}
	s, err := LoadSyncFrom(env(with(syncRequired(), map[string]string{EnvMigrateDSN: migrateDSNValue})))
	if err != nil {
		t.Fatalf("LoadSyncFrom: %v", err)
	}
	if s.MigrateDatabaseURL.Reveal() != migrateDSNValue {
		t.Error("LoadSyncFrom did not read the variable named by EnvMigrateDSN")
	}
	c, err := LoadFrom(env(with(required(), map[string]string{EnvMigrateDSN: migrateDSNValue})))
	if err != nil {
		t.Fatalf("LoadFrom: %v", err)
	}
	if c.MigrateDatabaseURL.Reveal() != migrateDSNValue {
		t.Error("LoadFrom did not read the variable named by EnvMigrateDSN")
	}
}
