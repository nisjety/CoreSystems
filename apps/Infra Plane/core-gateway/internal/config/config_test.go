package config

import (
	"os"
	"regexp"
	"testing"

	"github.com/spf13/viper"
)

// TestSetDefaultsHasNoDatabaseDSN keeps a Postgres DSN from returning as a
// default. A default DSN would put a credential in source and would point any
// boot that leaves DATABASE_POSTGRESURL unset at a legacy database.
func TestSetDefaultsHasNoDatabaseDSN(t *testing.T) {
	viper.Reset()
	t.Cleanup(viper.Reset)
	setDefaults()

	// Do not print the value: if a default comes back, the value is a credential.
	if got := viper.GetString("database.postgresurl"); got != "" {
		t.Fatal("database.postgresurl has a non-empty default; the DSN must come from DATABASE_POSTGRESURL")
	}
}

// TestConfigSourceEmbedsNoDatabasePassword scans config.go for a Postgres URL
// whose userinfo carries a password. A match is reported by byte offset only,
// so the test never prints the credential it finds.
func TestConfigSourceEmbedsNoDatabasePassword(t *testing.T) {
	src, err := os.ReadFile("config.go")
	if err != nil {
		t.Fatalf("read config.go: %v", err)
	}
	passwordDSN := regexp.MustCompile(`postgres(ql)?://[^/@\s]+:[^/@\s]+@`)
	if loc := passwordDSN.FindIndex(src); loc != nil {
		t.Fatalf("config.go embeds a Postgres URL with a password at byte offset %d", loc[0])
	}
}

// TestLoadReadsDatabaseURLFromEnvironment shows that DATABASE_POSTGRESURL still
// reaches the config once the embedded default is gone. viper.Unmarshal decodes
// only keys viper already knows, and AutomaticEnv does not add one, so the
// empty default is what keeps the env var working.
func TestLoadReadsDatabaseURLFromEnvironment(t *testing.T) {
	// A placeholder without a credential. Nothing connects to it.
	const dsn = "postgres://gateway@db.example.test:5432/coresystem?sslmode=disable"
	t.Setenv("DATABASE_POSTGRESURL", dsn)
	viper.Reset()
	t.Cleanup(viper.Reset)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.Database.PostgresURL != dsn {
		t.Fatalf("Database.PostgresURL = %q, want the DATABASE_POSTGRESURL value", cfg.Database.PostgresURL)
	}
}

// TestLoadLeavesDatabaseURLEmptyWhenUnset checks that a boot without the env
// var gets no DSN at all, rather than the old embedded default.
func TestLoadLeavesDatabaseURLEmptyWhenUnset(t *testing.T) {
	t.Setenv("DATABASE_POSTGRESURL", "")
	viper.Reset()
	t.Cleanup(viper.Reset)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.Database.PostgresURL != "" {
		t.Fatal("Database.PostgresURL is set without DATABASE_POSTGRESURL; there must be no default DSN")
	}
}
