package db_test

import (
	"strings"
	"testing"

	"github.com/triodelab/anbud/internal/db"
)

// H3: a DSN whose host is not loopback must verify the server certificate, which
// means sslmode verify-ca or verify-full. sslmode require encrypts but does not
// verify, so it is refused. The check is pure: it reads the parsed configuration
// and touches no network. cmd/doffin-sync and cmd/doffin-probe both apply it.
func TestCheckTLSRequirement(t *testing.T) {
	t.Parallel()

	cases := []struct {
		name    string
		dsn     string
		wantErr bool
	}{
		{"loopback 127.0.0.1 without TLS passes", "postgres://anbud:tls-marker-pw@127.0.0.1:5432/anbud?sslmode=disable", false},
		{"loopback localhost without TLS passes", "postgres://anbud:tls-marker-pw@localhost:5432/anbud?sslmode=disable", false},
		{"loopback ::1 without TLS passes", "postgres://anbud:tls-marker-pw@[::1]:5432/anbud?sslmode=disable", false},
		{"unix socket directory passes", "host=/var/run/postgresql dbname=anbud user=anbud password=tls-marker-pw", false},
		{"remote with prefer is refused", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=prefer", true},
		{"remote with the default sslmode is refused", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud", true},
		{"remote with require is refused", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=require", true},
		{"remote with require and sslrootcert=system passes, since pgx verifies it as verify-full", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=require&sslrootcert=system", false},
		{"remote with verify-ca passes", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=verify-ca", false},
		{"remote with verify-full passes", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=verify-full", false},
		{"remote with disable is refused", "postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=disable", true},
		{"keyword form remote with require is refused", "host=db.example.com port=5432 user=anbud password=tls-marker-pw dbname=anbud sslmode=require", true},
		{"keyword form remote with verify-ca passes", "host=db.example.com port=5432 user=anbud password=tls-marker-pw dbname=anbud sslmode=verify-ca", false},
		{"keyword form remote with verify-full passes", "host=db.example.com port=5432 user=anbud password=tls-marker-pw dbname=anbud sslmode=verify-full", false},
		{"keyword form remote with disable is refused", "host=db.example.com port=5432 user=anbud password=tls-marker-pw dbname=anbud sslmode=disable", true},
		{"keyword form remote with prefer is refused", "host=db.example.com port=5432 user=anbud password=tls-marker-pw dbname=anbud sslmode=prefer", true},
		{"keyword form loopback without TLS passes", "host=127.0.0.1 port=5432 user=anbud password=tls-marker-pw dbname=anbud sslmode=disable", false},
		{"several remote hosts are refused, since a fallback may be plaintext", "host=db1.example.com,db2.example.com sslmode=require user=anbud password=tls-marker-pw dbname=anbud", true},
		{"a remote host behind a loopback host is still checked", "host=127.0.0.1,db.example.com sslmode=disable user=anbud password=tls-marker-pw dbname=anbud", true},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			t.Parallel()

			err := db.CheckTLS(c.dsn)
			if (err != nil) != c.wantErr {
				t.Fatalf("CheckTLS error = %v, want error %v", err, c.wantErr)
			}
			if err == nil {
				return
			}
			if !strings.Contains(err.Error(), "ANBUD_DSN") {
				t.Errorf("error %q does not name ANBUD_DSN", err.Error())
			}
			if strings.Contains(err.Error(), "tls-marker-pw") || strings.Contains(err.Error(), "db.example.com") {
				t.Errorf("error %q echoes part of the DSN", err.Error())
			}
		})
	}
}

// An unparsable DSN is refused, and the parser's own message, which can quote the
// DSN, is not passed on.
func TestCheckTLSRefusesUnparsableWithoutEcho(t *testing.T) {
	t.Parallel()

	err := db.CheckTLS("postgres://anbud:tls-marker-pw@db.example.com:notaport/anbud?sslmode=require")
	if err == nil {
		t.Fatal("CheckTLS accepted an unparsable DSN")
	}
	if strings.Contains(err.Error(), "tls-marker-pw") {
		t.Errorf("error %q echoes the password", err.Error())
	}
	if !strings.Contains(err.Error(), "ANBUD_DSN") {
		t.Errorf("error %q does not name ANBUD_DSN", err.Error())
	}
}

// The two refusals have fixed texts: doffin-sync and doffin-probe print them as
// they are, and an operator may search for them. They name the variable and the
// requirement and nothing from the DSN.
func TestCheckTLSErrorTexts(t *testing.T) {
	t.Parallel()

	const (
		wantRefused    = "ANBUD_DSN names a host that is not loopback, so it must verify the server certificate: sslmode=verify-ca or verify-full, with exactly one host"
		wantUnparsable = "ANBUD_DSN is not a valid connection string"
	)
	refused := db.CheckTLS("postgres://anbud:tls-marker-pw@db.example.com:5432/anbud?sslmode=require")
	if refused == nil || refused.Error() != wantRefused {
		t.Errorf("remote DSN without verification: error = %v, want %q", refused, wantRefused)
	}
	unparsable := db.CheckTLS("postgres://anbud:tls-marker-pw@db.example.com:notaport/anbud?sslmode=require")
	if unparsable == nil || unparsable.Error() != wantUnparsable {
		t.Errorf("unparsable DSN: error = %v, want %q", unparsable, wantUnparsable)
	}
}
