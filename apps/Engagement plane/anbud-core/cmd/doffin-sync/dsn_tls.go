package main

import "github.com/triodelab/anbud/internal/db"

// checkDatabaseTLS applies the shared strict TLS rule for database connections to
// the sync's DSN: a host that is not loopback must verify the server certificate.
// The rule and its tests live in internal/db (db.CheckTLS), because doffin-probe
// applies the same rule before it takes the budget lock. The error names ANBUD_DSN
// and the rule and never echoes the DSN.
func checkDatabaseTLS(dsn string) error { return db.CheckTLS(dsn) }
