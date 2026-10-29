package main

import (
	"testing"

	"github.com/triodelab/anbud/internal/db"
)

// The strict TLS rule for database connections lives in internal/db (db.CheckTLS),
// with its table, so that doffin-sync and doffin-probe apply one rule. checkDatabaseTLS
// is a thin call to it. This test pins that it adds and changes nothing: a DSN that
// passes, one that is refused for its TLS mode and one that does not parse give the
// shared rule's answer, error text included.
func TestCheckDatabaseTLSDelegatesToTheSharedRule(t *testing.T) {
	cases := []struct {
		name string
		dsn  string
	}{
		{"a loopback DSN passes", "postgres://anbud:tls-marker-pw@127.0.0.1:5432/anbud?sslmode=disable"},
		{"a remote DSN that does not verify is refused", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=require"},
		{"a remote DSN that verifies passes", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=verify-full"},
		{"an unparsable DSN is refused", "postgres://anbud:tls-marker-pw@db.example.com:notaport/anbud?sslmode=require"},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			got, want := checkDatabaseTLS(c.dsn), db.CheckTLS(c.dsn)
			if (got == nil) != (want == nil) {
				t.Fatalf("checkDatabaseTLS error = %v, db.CheckTLS error = %v", got, want)
			}
			if got != nil && got.Error() != want.Error() {
				t.Errorf("checkDatabaseTLS error text = %q, want the shared rule's %q", got.Error(), want.Error())
			}
		})
	}
}
