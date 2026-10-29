package db_test

import (
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
)

// ANBUD_MIGRATE_DSN goes through the same strict TLS rule as ANBUD_DSN (O2). The
// rule is db.CheckTLS with the variable's name in its messages, so an operator is
// told which of the two DSNs to fix. The check is pure.

const migrateVar = "ANBUD_MIGRATE_DSN"

func TestCheckTLSForAppliesTheSameRuleAndNamesTheVariable(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		dsn     string
		wantErr bool
	}{
		{"loopback without TLS passes", "postgres://anbud_migrator:tls-marker-pw@127.0.0.1:5432/anbud?sslmode=disable", false},
		{"a Unix socket directory passes", "host=/var/run/postgresql dbname=anbud user=anbud_migrator password=tls-marker-pw", false},
		{"remote with verify-full passes", "postgres://anbud_migrator:tls-marker-pw@db.example.com:5432/anbud?sslmode=verify-full", false},
		{"remote with verify-ca passes", "postgres://anbud_migrator:tls-marker-pw@db.example.com:5432/anbud?sslmode=verify-ca", false},
		{"remote with require is refused", "postgres://anbud_migrator:tls-marker-pw@db.example.com:5432/anbud?sslmode=require", true},
		{"remote with the default sslmode is refused", "postgres://anbud_migrator:tls-marker-pw@db.example.com:5432/anbud", true},
		{"remote with disable is refused", "postgres://anbud_migrator:tls-marker-pw@db.example.com:5432/anbud?sslmode=disable", true},
		{"several hosts, one remote, are refused", "host=127.0.0.1,db.example.com sslmode=disable user=anbud_migrator password=tls-marker-pw dbname=anbud", true},
		{"an unparsable DSN is refused", "postgres://anbud_migrator:tls-marker-pw@db.example.com:notaport/anbud?sslmode=require", true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			t.Parallel()

			err := db.CheckTLSFor(migrateVar, c.dsn)
			if (err != nil) != c.wantErr {
				t.Fatalf("CheckTLSFor error = %v, want error %v", err, c.wantErr)
			}
			// The verdict is the one CheckTLS gives ANBUD_DSN: one rule, two names.
			if shared := db.CheckTLS(c.dsn); (shared != nil) != c.wantErr {
				t.Fatalf("CheckTLS disagrees: %v", shared)
			}
			if err == nil {
				return
			}
			if !strings.HasPrefix(err.Error(), migrateVar+" ") {
				t.Errorf("error %q does not start with %s", err.Error(), migrateVar)
			}
			if strings.Contains(err.Error(), "ANBUD_DSN") {
				t.Errorf("error %q names the wrong variable", err.Error())
			}
			for _, leak := range []string{"tls-marker-pw", "db.example.com", "anbud_migrator", c.dsn} {
				if strings.Contains(err.Error(), leak) {
					t.Errorf("error %q echoes part of the DSN (%q)", err.Error(), leak)
				}
			}
		})
	}
}

// The two refusals have fixed texts, as CheckTLS's have: the same sentences with
// the variable's name, and nothing from the DSN.
func TestCheckTLSForErrorTexts(t *testing.T) {
	t.Parallel()

	const (
		wantRefused    = "ANBUD_MIGRATE_DSN names a host that is not loopback, so it must verify the server certificate: sslmode=verify-ca or verify-full, with exactly one host"
		wantUnparsable = "ANBUD_MIGRATE_DSN is not a valid connection string"
	)
	refused := db.CheckTLSFor(migrateVar, "postgres://anbud_migrator:tls-marker-pw@db.example.com:5432/anbud?sslmode=require")
	if refused == nil || refused.Error() != wantRefused {
		t.Errorf("remote DSN without verification: error = %v, want %q", refused, wantRefused)
	}
	unparsable := db.CheckTLSFor(migrateVar, "postgres://anbud_migrator:tls-marker-pw@db.example.com:notaport/anbud?sslmode=require")
	if unparsable == nil || unparsable.Error() != wantUnparsable {
		t.Errorf("unparsable DSN: error = %v, want %q", unparsable, wantUnparsable)
	}
}

// CheckTLS is CheckTLSFor for ANBUD_DSN, with the texts it always had.
func TestCheckTLSIsCheckTLSForANBUDDSN(t *testing.T) {
	t.Parallel()

	for _, dsn := range []string{
		"postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=require",
		"postgres://anbud:tls-marker-pw@db.example.com:notaport/anbud?sslmode=require",
		"postgres://anbud:tls-marker-pw@127.0.0.1:5432/anbud?sslmode=disable",
	} {
		want, got := db.CheckTLSFor("ANBUD_DSN", dsn), db.CheckTLS(dsn)
		if (want == nil) != (got == nil) || (want != nil && want.Error() != got.Error()) {
			t.Errorf("CheckTLS = %v, CheckTLSFor(ANBUD_DSN) = %v", got, want)
		}
	}
}
