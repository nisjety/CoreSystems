package db_test

import (
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db/dbtest"
)

// A DSN whose database is anbud_core must never reach a database test. The
// refusal must not echo the DSN, so the password and host stay out of the error.
func TestDSNGuardRefusesAnbudCore(t *testing.T) {
	t.Parallel()

	const dsn = "postgres://anbud:hunter2-guard-test@db.internal.example:5432/anbud_core?sslmode=disable"
	err := dbtest.CheckDSN(dsn)
	if err == nil {
		t.Fatal("CheckDSN accepted a DSN for anbud_core")
	}
	for _, leak := range []string{"hunter2-guard-test", "db.internal.example", dsn} {
		if strings.Contains(err.Error(), leak) {
			t.Fatalf("refusal echoed %q: %v", leak, err)
		}
	}
}

// Only the postgres database and names with the anbud_test prefix are allowed.
func TestDSNGuardAllowsOnlyThrowawayDatabases(t *testing.T) {
	t.Parallel()

	tests := []struct {
		database string
		allowed  bool
	}{
		{"postgres", true},
		{"anbud_test", true},
		{"anbud_test_3f9a1c2b", true},
		{"anbud_core", false},
		{"anbud", false},
		{"Postgres", false},
		{"postgres2", false},
		{"ANBUD_TEST_X", false},
		{"", false},
	}
	for _, tc := range tests {
		t.Run(tc.database, func(t *testing.T) {
			t.Parallel()

			dsn := "postgres://postgres:pw@127.0.0.1:5432/" + tc.database + "?sslmode=disable"
			err := dbtest.CheckDSN(dsn)
			if tc.allowed && err != nil {
				t.Fatalf("database %q refused: %v", tc.database, err)
			}
			if !tc.allowed && err == nil {
				t.Fatalf("database %q accepted", tc.database)
			}
		})
	}
}

// A DSN that does not parse is refused, and the refusal does not echo it.
func TestDSNGuardRefusesAnUnparsableDSNWithoutEchoingIt(t *testing.T) {
	t.Parallel()

	const dsn = "postgres://u:unparsable-secret-pw@host:notaport/db"
	err := dbtest.CheckDSN(dsn)
	if err == nil {
		t.Fatal("CheckDSN accepted an unparsable DSN")
	}
	if strings.Contains(err.Error(), "unparsable-secret-pw") {
		t.Fatalf("refusal echoed the password: %v", err)
	}
}
