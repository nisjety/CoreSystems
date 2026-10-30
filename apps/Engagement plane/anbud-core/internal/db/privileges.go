package db

import _ "embed"

// privilegesSQL is the application role's table privileges, stated again at the end
// of every migration run, inside the migration transaction, by the role that owns
// the tables. scripts/roles.sql, which a DBA runs once, names no table, so it cannot
// give the privileges that only a table can have: UPDATE on the two ledger tables
// needs the tables to exist, and on a new database they do not until the first
// migration has run. The runner states them every time instead, so a grant lost or
// added by hand is corrected at the next start, and the privileges travel with the
// binary that knows the tables. The statements do nothing when the role does not
// exist, which is how a database without the role model migrates as it always did.
// docs/ROLES.md has the matrix this produces.
//
//go:embed privileges.sql
var privilegesSQL string

// tenantPrivilegesSQL is the same for the schema tenant of migration 0007, which holds a tenant's
// uploaded tender documents and is not evidence. The runner states it after privileges.sql, in the same
// transaction.
//
//go:embed tenant_privileges.sql
var tenantPrivilegesSQL string
