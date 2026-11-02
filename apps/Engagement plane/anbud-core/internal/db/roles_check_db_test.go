package db_test

import (
	"context"
	"errors"
	"reflect"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/anbud/internal/db"
	"github.com/triodelab/anbud/internal/db/dbtest"
)

// The runtime-role check against real roles. The decision table is pure
// (rolecheck_test.go); this proves the query reads the right catalog columns for each
// criterion on its own, by connecting as roles that have exactly one. Every reason the check
// has, membership and ownership included, is in rolecheck_reasons_db_test.go. Needs a
// throwaway server: run with make test-db.

func TestRuntimeRoleCheckReadsEachCriterionFromTheCatalog(t *testing.T) {
	rs, admin := migratedRoles(t)
	ctx := roleCtx(t)

	// What the owner role holds: its own role, every relation and function the migrations
	// made, the database it owns and, from PostgreSQL 15, the schema public, which the
	// database's owner owns through pg_database_owner.
	owner := db.RoleFacts{
		HoldsMigratorRole: true, OwnsRelation: true, OwnsFunction: true, OwnsDatabase: true,
		OwnsSchema: schemaIsOwnedByTheDatabaseOwner(ctx, t, admin),
	}
	cases := []struct {
		name string
		dsn  string
		want db.RoleFacts
	}{
		{"the application role", rs.AppDSN, db.RoleFacts{}},
		{"the owner role, which owns the objects, the schema and the database", rs.MigratorDSN, owner},
		{"a role with CREATEROLE only", dbtest.CreateLoginRole(t, rs, "anbud_test_createrole", "CREATEROLE"), db.RoleFacts{CreateRole: true}},
		{"a role with CREATEDB only", dbtest.CreateLoginRole(t, rs, "anbud_test_createdb", "CREATEDB"), db.RoleFacts{CreateDB: true}},
		{"a role with BYPASSRLS only", dbtest.CreateLoginRole(t, rs, "anbud_test_bypassrls", "BYPASSRLS"), db.RoleFacts{BypassRLS: true}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			pool := dbtest.Pool(t, c.dsn)

			got, err := db.ReadRoleFacts(ctx, pool)
			if err != nil {
				t.Fatalf("ReadRoleFacts: %s", dbtest.Describe(err))
			}
			if !reflect.DeepEqual(got, c.want) {
				t.Fatalf("facts = %+v, want %+v", got, c.want)
			}

			// On loopback a role with any fact is a warning, and on a remote host a refusal.
			verdict, err := db.CheckRuntimeRole(ctx, pool, c.dsn)
			switch {
			case !c.want.Excessive() && (err != nil || verdict != db.RoleOK):
				t.Errorf("loopback: got (%v, %v), want (RoleOK, nil)", verdict, err)
			case c.want.Excessive() && (err != nil || verdict != db.RoleWarn):
				t.Errorf("loopback: got (%v, %v), want (RoleWarn, nil)", verdict, err)
			}
			verdict, err = db.CheckRuntimeRole(ctx, pool, remoteDSN)
			switch {
			case !c.want.Excessive() && (err != nil || verdict != db.RoleOK):
				t.Errorf("remote: got (%v, %v), want (RoleOK, nil)", verdict, err)
			case c.want.Excessive() && !errors.Is(err, db.ErrRoleTooPrivileged):
				t.Errorf("remote: error = %v, want ErrRoleTooPrivileged", err)
			}
		})
	}
}

// schemaIsOwnedByTheDatabaseOwner says whether the schema public belongs to
// pg_database_owner, as it does on a database made on PostgreSQL 15 or later.
func schemaIsOwnedByTheDatabaseOwner(ctx context.Context, t *testing.T, admin *pgxpool.Pool) bool {
	t.Helper()
	return scanBool(ctx, t, admin, `SELECT coalesce(n.nspowner = to_regrole('pg_database_owner')::oid, false) FROM pg_namespace n WHERE n.nspname = 'public'`)
}

// The server's own role in the throwaway cluster is a superuser: refused on a remote
// host, a warning on loopback. Every role it inherits is a superuser too, and its other
// facts are whatever the cluster makes them, so only the one that matters is asserted.
func TestRuntimeRoleCheckRefusesARealSuperuserOffLoopbackAndWarnsOnLoopback(t *testing.T) {
	rs, _ := migratedRoles(t)
	ctx := roleCtx(t)
	pool := dbtest.Pool(t, rs.AdminDSN)

	facts, err := db.ReadRoleFacts(ctx, pool)
	if err != nil {
		t.Fatalf("ReadRoleFacts: %s", dbtest.Describe(err))
	}
	if !facts.Superuser {
		t.Fatalf("facts = %+v, want a superuser", facts)
	}
	if _, err := db.CheckRuntimeRole(ctx, pool, remoteDSN); !errors.Is(err, db.ErrRoleTooPrivileged) {
		t.Errorf("a superuser on a remote host: error = %v, want ErrRoleTooPrivileged", err)
	}
	if verdict, err := db.CheckRuntimeRole(ctx, pool, rs.AdminDSN); err != nil || verdict != db.RoleWarn {
		t.Errorf("a superuser on loopback: got (%v, %v), want (RoleWarn, nil)", verdict, err)
	}
}
