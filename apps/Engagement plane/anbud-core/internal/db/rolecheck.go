package db

import (
	"context"
	"errors"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// The append-only triggers on the evidence tables stop a mistake, not a role that owns
// the tables or is a superuser: such a role can drop a trigger, switch it off or set
// session_replication_role. They are a security boundary only when the role the
// engine writes through can do none of that. docs/ROLES.md describes the role model
// that makes it so; this file is the check that the role in use is one of those.
//
// A role is judged by what it can use or become, not only by what it is. The
// attributes (superuser, CREATEROLE, CREATEDB, BYPASSRLS) count when any role it is a
// member of holds them, because a member can SET ROLE to it, whether or not it inherits;
// ownership counts when any role it is a member of owns the object, for every kind of
// relation and for functions, the schema it resolves names in and the database itself;
// and membership of a predefined role that reaches files, programs or other sessions, or
// of one of the model's own roles (the owner and the DBA group, which the application must
// belong to neither of), counts on its own. Any one fact is enough.
//
// The rule: if the runtime role has any fact, a command refuses to start when the DSN
// names a host that is not loopback, and on a loopback host it prints one warning line
// and goes on. Loopback is judged exactly as CheckTLS judges it.
//
// The role that owns the schema (ANBUD_MIGRATE_DSN) is judged by the same facts, but only
// by those that let a role leave the database: it owns everything, and holds the owner
// role, by design. See CheckOwnerRole.

// ErrRoleTooPrivileged is the refusal: the role is too privileged for a host that is
// not loopback. Its text is fixed and carries nothing from the DSN, and no role name.
var ErrRoleTooPrivileged = errors.New("ANBUD_DSN names a host that is not loopback and a database role that is too privileged: the role, and every role it is a member of, must not be a superuser or hold CREATEROLE, CREATEDB or BYPASSRLS; it must not be a member of a server-wide role that reaches files, programs or other sessions, or of the owner or DBA roles of the role model; and it must not own a table, view, sequence, function, schema or the database; run the migrations through ANBUD_MIGRATE_DSN as a separate owner role (docs/ROLES.md)")

// ErrRoleCheckFailed is returned when the role could not be read. The check fails
// closed: a start that cannot tell what its role may do does not go on. The driver's
// own error is not passed on, because it can quote the connection details.
var ErrRoleCheckFailed = errors.New("could not check the privileges of the database role in ANBUD_DSN")

// RoleWarning is the one line a command prints, after its own name, when the role
// is too privileged and the host is loopback. It names no role and no DSN.
const RoleWarning = "warning: the database role in ANBUD_DSN is more privileged than the sync needs (a superuser, one that holds CREATEROLE, CREATEDB or BYPASSRLS itself or through a role it is a member of, a member of a server-wide role that reaches files, programs or other sessions or of the owner or DBA roles of the role model, or an owner of database objects), so it may be able to switch off the append-only triggers; this is accepted on a loopback host only (docs/ROLES.md)"

// ErrOwnerTooPrivileged is the refusal for the role of ANBUD_MIGRATE_DSN: it is a superuser,
// holds CREATEROLE, CREATEDB or BYPASSRLS, or belongs to a predefined role that reaches
// files, programs or other sessions, and the host is not loopback. The text is fixed.
var ErrOwnerTooPrivileged = errors.New("ANBUD_MIGRATE_DSN names a host that is not loopback and a database role that is too privileged: the role, and every role it is a member of, must not be a superuser or hold CREATEROLE, CREATEDB or BYPASSRLS, and it must not be a member of a server-wide role that reaches files, programs or other sessions; the owner role needs to own the schema and nothing more (docs/ROLES.md)")

// ErrOwnerCheckFailed is returned when the owner role could not be read. It fails closed,
// like ErrRoleCheckFailed, and carries none of the driver's text.
var ErrOwnerCheckFailed = errors.New("could not check the privileges of the database role in ANBUD_MIGRATE_DSN")

// OwnerWarning is the one line printed when the owner role is too privileged and the host is
// loopback. It names no role and no DSN.
const OwnerWarning = "warning: the database role in ANBUD_MIGRATE_DSN is more privileged than the migrations need (a superuser, one that holds CREATEROLE, CREATEDB or BYPASSRLS itself or through a role it is a member of, or a member of a server-wide role that reaches files, programs or other sessions); this is accepted on a loopback host only (docs/ROLES.md)"

// RoleFacts is what the check reads about the role of a pool, one field for each reason
// the role may be too privileged. "Holds" and "owns" are read through every role the role is a
// member of, itself included. ReadRoleFacts scans one column of its query into each field, in
// the order the fields are declared in, and every field is a bool.
type RoleFacts struct {
	Superuser           bool // the role, or a role it is a member of, is a superuser
	CreateRole          bool // ... holds CREATEROLE
	CreateDB            bool // ... holds CREATEDB
	BypassRLS           bool // ... holds BYPASSRLS
	HoldsPredefinedRole bool // it is a member of a predefined role that reaches files, programs or other sessions
	HoldsMigratorRole   bool // it is the owner role of the model, or a member of it
	HoldsDBARole        bool // it is the DBA group of the model, or a member of it
	OwnsRelation        bool // it owns a table, view, materialized view, sequence or foreign table, in any schema
	OwnsFunction        bool // it owns a function or a procedure
	OwnsSchema          bool // it owns a schema on its search path
	OwnsDatabase        bool // it owns the database it is connected to
}

// Excessive reports whether any one fact is true. Each is enough on its own: any of
// them lets the role undo the boundary, or give itself the power to.
func (f RoleFacts) Excessive() bool {
	return f.Superuser || f.CreateRole || f.CreateDB || f.BypassRLS ||
		f.HoldsPredefinedRole || f.HoldsMigratorRole || f.HoldsDBARole ||
		f.OwnsRelation || f.OwnsFunction || f.OwnsSchema || f.OwnsDatabase
}

// ExcessiveAsOwner reports whether the facts make a role too privileged to own the schema.
// The owner owns every object, the schema and the database by design, and holds the owner
// role, so ownership and the model's own roles are not reasons. What is: the attributes
// that let a role leave the database or make others, and a predefined role that reaches
// files, programs or other sessions.
func (f RoleFacts) ExcessiveAsOwner() bool {
	return f.Superuser || f.CreateRole || f.CreateDB || f.BypassRLS || f.HoldsPredefinedRole
}

// RoleVerdict is the outcome of a check that did not refuse.
type RoleVerdict int

const (
	// RoleOK means the role holds nothing it should not.
	RoleOK RoleVerdict = iota
	// RoleWarn means the role is too privileged but the host is loopback: the
	// command prints RoleWarning and goes on.
	RoleWarn
)

func (v RoleVerdict) String() string {
	switch v {
	case RoleOK:
		return "RoleOK"
	case RoleWarn:
		return "RoleWarn"
	}
	return "RoleVerdict(?)"
}

// RowQuerier is the one method of a pool the check uses. A *pgxpool.Pool is one, and
// a test stands in a fake, so the decision runs without Postgres.
type RowQuerier interface {
	QueryRow(ctx context.Context, sql string, args ...any) pgx.Row
}

// roleFactsSQL reads the catalog for the role the session runs as. The roles in "held" are the
// ones the session's role is a member of, itself included. 'MEMBER' and not 'USAGE': USAGE is
// membership that inherits, and a role that is a member without inheriting still has the
// attributes of the group and the group's ownership one SET ROLE away, which is as good as having
// them. For a superuser pg_has_role is true for every role, so every fact is true, which is
// right: it can do all of it.
//
// The relations counted are tables, partitioned tables, views, materialized views,
// foreign tables and sequences in any schema, and not the temporary ones, which belong to a
// session and go with it. Functions include procedures. A schema counts when it is on the
// role's search path (current_schemas), the ones an unqualified name resolves in; the
// schema public is owned by pg_database_owner from PostgreSQL 15, so the owner of the
// database owns it too. The predefined roles are the ones that can run programs
// (pg_execute_server_program), read or write the server's files (pg_read_server_files,
// pg_write_server_files), write every table (pg_write_all_data), end other sessions
// (pg_signal_backend), maintain every table (pg_maintain, PostgreSQL 17) and force a checkpoint
// (pg_checkpoint). The ones a server does not have are never in pg_roles.
const roleFactsSQL = `
WITH held AS (
    SELECT r.oid, r.rolname, r.rolsuper, r.rolcreaterole, r.rolcreatedb, r.rolbypassrls
      FROM pg_roles r
     WHERE pg_has_role(current_user, r.oid, 'MEMBER')
)
SELECT COALESCE(bool_or(h.rolsuper), false),
       COALESCE(bool_or(h.rolcreaterole), false),
       COALESCE(bool_or(h.rolcreatedb), false),
       COALESCE(bool_or(h.rolbypassrls), false),
       COALESCE(bool_or(h.rolname IN ('pg_execute_server_program', 'pg_write_server_files', 'pg_read_server_files',
                                       'pg_write_all_data', 'pg_signal_backend', 'pg_maintain', 'pg_checkpoint')), false),
       COALESCE(bool_or(h.rolname = 'anbud_migrator'), false),
       COALESCE(bool_or(h.rolname = 'anbud_dba'), false),
       EXISTS (SELECT 1 FROM pg_class c
                WHERE c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
                  AND c.relpersistence <> 't'
                  AND c.relowner IN (SELECT oid FROM held)),
       EXISTS (SELECT 1 FROM pg_proc p WHERE p.proowner IN (SELECT oid FROM held)),
       EXISTS (SELECT 1 FROM pg_namespace n
                WHERE n.nspname = ANY (current_schemas(false))
                  AND n.nspowner IN (SELECT oid FROM held)),
       EXISTS (SELECT 1 FROM pg_database d
                WHERE d.datname = current_database()
                  AND d.datdba IN (SELECT oid FROM held))
  FROM held h`

// ReadRoleFacts asks the database what the pool's role may do. It returns the
// driver's error as it is: CheckRuntimeRole turns it into the fixed ErrRoleCheckFailed.
func ReadRoleFacts(ctx context.Context, q RowQuerier) (RoleFacts, error) {
	var f RoleFacts
	err := q.QueryRow(ctx, roleFactsSQL).Scan(
		&f.Superuser, &f.CreateRole, &f.CreateDB, &f.BypassRLS,
		&f.HoldsPredefinedRole, &f.HoldsMigratorRole, &f.HoldsDBARole,
		&f.OwnsRelation, &f.OwnsFunction, &f.OwnsSchema, &f.OwnsDatabase)
	if err != nil {
		return RoleFacts{}, err
	}
	return f, nil
}

// DecideRole is the decision table. A role with no fact passes anywhere. A role with
// any fact is a warning on a loopback host and a refusal on any other.
func DecideRole(f RoleFacts, remote bool) (RoleVerdict, error) {
	switch {
	case !f.Excessive():
		return RoleOK, nil
	case remote:
		return RoleOK, ErrRoleTooPrivileged
	default:
		return RoleWarn, nil
	}
}

// DecideOwnerRole is the decision table for the role of ANBUD_MIGRATE_DSN: the same, on the
// facts that count for an owner (ExcessiveAsOwner).
func DecideOwnerRole(f RoleFacts, remote bool) (RoleVerdict, error) {
	switch {
	case !f.ExcessiveAsOwner():
		return RoleOK, nil
	case remote:
		return RoleOK, ErrOwnerTooPrivileged
	default:
		return RoleWarn, nil
	}
}

// CheckRuntimeRole judges the role of the pool the engine writes through. q is that
// pool and dsn is the DSN it was opened from; the DSN is read only to tell whether
// its host is loopback, by the rule CheckTLS applies, and never leaves this function.
// The error is ErrRoleTooPrivileged for a refusal and ErrRoleCheckFailed when the
// role could not be read. A DSN that does not parse counts as a remote host.
func CheckRuntimeRole(ctx context.Context, q RowQuerier, dsn string) (RoleVerdict, error) {
	facts, err := ReadRoleFacts(ctx, q)
	if err != nil {
		return RoleOK, ErrRoleCheckFailed
	}
	return DecideRole(facts, dsnIsRemote(dsn))
}

// CheckOwnerRole judges the role of the pool the migrations run through, the same way:
// q is that pool, dsn the DSN of ANBUD_MIGRATE_DSN, read only for its host. The error is
// ErrOwnerTooPrivileged for a refusal and ErrOwnerCheckFailed when the role could not be read.
func CheckOwnerRole(ctx context.Context, q RowQuerier, dsn string) (RoleVerdict, error) {
	facts, err := ReadRoleFacts(ctx, q)
	if err != nil {
		return RoleOK, ErrOwnerCheckFailed
	}
	return DecideOwnerRole(facts, dsnIsRemote(dsn))
}

// dsnIsRemote applies CheckTLS's judgement of a host: a host that is not localhost,
// 127.0.0.1, ::1 or a Unix socket directory, in the DSN or among its fallbacks, makes
// the DSN remote. There is no second rule: it is hasRemoteHost. A DSN that does not
// parse is judged remote, so a check that cannot tell fails closed.
func dsnIsRemote(dsn string) bool {
	cfg, err := pgconn.ParseConfig(dsn)
	if err != nil {
		return true
	}
	return hasRemoteHost(cfg)
}
