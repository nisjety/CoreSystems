package db

import (
	"crypto/tls"
	"errors"
	"strings"

	"github.com/jackc/pgx/v5/pgconn"
)

// CheckTLS refuses a DSN that would send the database password to a host that is
// not local unless the server's certificate is verified. A host is local when it is
// localhost, 127.0.0.1, ::1 or a Unix socket directory. If any host in the DSN,
// primary or fallback, is not local, the primary must verify the server
// certificate, and the DSN must name exactly one host, so no plaintext fallback can
// be used. The error names ANBUD_DSN and the rule. It never echoes the DSN, because
// parser errors can quote it.
//
// Every command that opens a database connection from ANBUD_DSN applies this rule
// before it connects: doffin-sync and doffin-probe.
func CheckTLS(dsn string) error { return CheckTLSFor("ANBUD_DSN", dsn) }

// CheckTLSFor is CheckTLS for the DSN that variable holds: the same rule, with the
// variable's name in its messages, so an operator is told which DSN to fix.
// doffin-sync applies it to ANBUD_MIGRATE_DSN as well as to ANBUD_DSN. The name is
// the caller's constant and never part of the DSN.
func CheckTLSFor(variable, dsn string) error {
	cfg, err := pgconn.ParseConfig(dsn)
	if err != nil {
		return errors.New(variable + " is not a valid connection string")
	}
	if !hasRemoteHost(cfg) {
		return nil
	}
	if cfg.TLSConfig == nil || !verifiesServer(cfg.TLSConfig) || len(cfg.Fallbacks) > 0 {
		return errors.New(variable + " names a host that is not loopback, so it must verify the server certificate: sslmode=verify-ca or verify-full, with exactly one host")
	}
	return nil
}

// verifiesServer reports whether pgx checks the server's certificate under this TLS
// configuration. verify-full leaves InsecureSkipVerify off. verify-ca keeps it on but
// installs its own chain check in VerifyPeerCertificate. require and prefer keep
// InsecureSkipVerify on with no check, so they encrypt without knowing who answered.
// pgconn does not expose the sslmode string, and it rewrites it (sslrootcert=system
// makes it verify-full), so the check reads what pgx will actually do.
func verifiesServer(tc *tls.Config) bool {
	return !tc.InsecureSkipVerify || tc.VerifyPeerCertificate != nil
}

// hasRemoteHost reports whether any host the DSN names is not local.
func hasRemoteHost(cfg *pgconn.Config) bool {
	if !isLocalHost(cfg.Host, cfg.Port) {
		return true
	}
	for _, fb := range cfg.Fallbacks {
		if !isLocalHost(fb.Host, fb.Port) {
			return true
		}
	}
	return false
}

// isLocalHost reports whether a parsed host stays on this machine. The loopback
// names are the three the plan lists, and a Unix socket directory is local too.
func isLocalHost(host string, port uint16) bool {
	if network, _ := pgconn.NetworkAddress(host, port); network == "unix" {
		return true
	}
	return strings.EqualFold(host, "localhost") || host == "127.0.0.1" || host == "::1"
}
