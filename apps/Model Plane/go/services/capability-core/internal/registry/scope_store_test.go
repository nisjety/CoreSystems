package registry

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

type scopeDatabaseStub struct {
	tag       pgconn.CommandTag
	boolValue bool
	err       error
	rows      pgx.Rows
	query     string
	args      []any
}

func (database *scopeDatabaseStub) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	database.query = query
	database.args = append([]any(nil), args...)
	return database.tag, database.err
}

func (database *scopeDatabaseStub) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	database.query = query
	database.args = append([]any(nil), args...)
	return database.rows, database.err
}

func (database *scopeDatabaseStub) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	database.query = query
	database.args = append([]any(nil), args...)
	return boolRow{value: database.boolValue, err: database.err}
}

type boolRow struct {
	value bool
	err   error
}

func (row boolRow) Scan(destinations ...any) error {
	if row.err != nil {
		return row.err
	}
	*(destinations[0].(*bool)) = row.value
	return nil
}

type scopeRows struct {
	rows  [][]any
	index int
	err   error
}

func (rows *scopeRows) Close()                                       {}
func (rows *scopeRows) Err() error                                   { return rows.err }
func (rows *scopeRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (rows *scopeRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (rows *scopeRows) RawValues() [][]byte                          { return nil }
func (rows *scopeRows) Conn() *pgx.Conn                              { return nil }
func (rows *scopeRows) Next() bool {
	rows.index++
	return rows.index < len(rows.rows)
}
func (rows *scopeRows) Values() ([]any, error) {
	if rows.index < 0 || rows.index >= len(rows.rows) {
		return nil, pgx.ErrNoRows
	}
	return append([]any(nil), rows.rows[rows.index]...), nil
}
func (rows *scopeRows) Scan(destinations ...any) error {
	values, err := rows.Values()
	if err != nil {
		return err
	}
	for index, destination := range destinations {
		target := reflect.ValueOf(destination).Elem()
		if values[index] == nil {
			target.SetZero()
			continue
		}
		target.Set(reflect.ValueOf(values[index]))
	}
	return nil
}

func TestScopeStoreGrantAndRevokePinVerifiedTenant(t *testing.T) {
	t.Parallel()
	database := &scopeDatabaseStub{tag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &ScopeStore{pool: database}

	grant, err := store.Grant(
		context.Background(), "grant-1", "org-a", "cap.agent", ScopeKindAgent, "shared-agent", "actor-a",
	)
	if err != nil {
		t.Fatalf("Grant: %v", err)
	}
	if grant.OrgID != "org-a" || len(database.args) != 7 || database.args[1] != "org-a" {
		t.Fatalf("tenant-bound grant = %+v, args = %#v", grant, database.args)
	}
	if !strings.Contains(database.query, "c.org_id = $2 OR c.org_id = 'global'") {
		t.Fatalf("grant query does not authorize tenant ownership: %s", database.query)
	}

	database.tag = pgconn.NewCommandTag("UPDATE 1")
	if _, err := store.Revoke(context.Background(), "org-a", "cap.agent", ScopeKindAgent, "shared-agent"); err != nil {
		t.Fatalf("Revoke: %v", err)
	}
	if len(database.args) != 4 || database.args[0] != "org-a" || !strings.Contains(database.query, "WHERE org_id = $1") {
		t.Fatalf("tenant-bound revoke query = %s, args = %#v", database.query, database.args)
	}
}

func TestScopeStoreResolutionChecksTenantBeforeCollidingAgentID(t *testing.T) {
	t.Parallel()
	database := &scopeDatabaseStub{boolValue: false}
	store := &ScopeStore{pool: database}

	granted, err := store.IsGrantedForScope(
		context.Background(), "cap.agent", "org-b", ScopeKindAgent, "shared-agent",
	)
	if err != nil || granted {
		t.Fatalf("foreign tenant grant = %v, err = %v", granted, err)
	}
	if len(database.args) != 4 || database.args[1] != "org-b" || database.args[3] != "shared-agent" {
		t.Fatalf("tenant-bound resolution args = %#v", database.args)
	}
	if !strings.Contains(database.query, "AND org_id = $2") {
		t.Fatalf("resolution query does not bind tenant: %s", database.query)
	}
}

func TestScopeStoreRejectsCallerSuppliedOrMalformedScopeAuthority(t *testing.T) {
	t.Parallel()
	database := &scopeDatabaseStub{tag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &ScopeStore{pool: database}

	for _, input := range []struct {
		orgID, kind, value string
	}{
		{orgID: "", kind: ScopeKindAgent, value: "agent-a"},
		{orgID: "org-a", kind: "something-random", value: "agent-a"},
		{orgID: "org-a", kind: ScopeKindOrg, value: "org-b"},
		{orgID: "org-a", kind: ScopeKindAgent, value: ""},
	} {
		if _, err := store.Grant(context.Background(), "", input.orgID, "cap.agent", input.kind, input.value, "actor"); err == nil {
			t.Fatalf("Grant(%+v) succeeded", input)
		}
	}
	if database.query != "" {
		t.Fatalf("malformed grants reached database: %s", database.query)
	}
}

func TestScopeStoreTenantBoundListAndResolve(t *testing.T) {
	t.Parallel()
	grantedAt := time.Date(2026, time.July, 13, 16, 0, 0, 0, time.UTC)
	database := &scopeDatabaseStub{rows: &scopeRows{index: -1, rows: [][]any{{
		"grant-1", "org-a", "cap.agent", ScopeKindAgent, "agent-a", "actor-a", grantedAt, nil,
	}}}}
	store := &ScopeStore{pool: database}

	grants, err := store.ListForCapabilityForOrg(context.Background(), "cap.agent", "org-a")
	if err != nil || len(grants) != 1 || grants[0].OrgID != "org-a" {
		t.Fatalf("tenant grants = %+v, err = %v", grants, err)
	}
	if len(database.args) != 2 || database.args[1] != "org-a" || !strings.Contains(database.query, "cs.org_id = $2") {
		t.Fatalf("tenant list query = %s, args = %#v", database.query, database.args)
	}

	database.rows = &scopeRows{index: -1, rows: [][]any{{"cap.agent"}}}
	resolved, err := store.ResolveForScopeForOrg(context.Background(), "org-a", ScopeKindAgent, "agent-a")
	if err != nil || len(resolved) != 1 || resolved[0] != "cap.agent" {
		t.Fatalf("tenant resolution = %v, err = %v", resolved, err)
	}
	if len(database.args) != 3 || database.args[2] != "org-a" || !strings.Contains(database.query, "cs.org_id = $3") {
		t.Fatalf("tenant resolve query = %s, args = %#v", database.query, database.args)
	}
}

func TestScopeStoreFailsClosedOnMissingRowsAndDatabaseErrors(t *testing.T) {
	t.Parallel()
	database := &scopeDatabaseStub{}
	store := &ScopeStore{pool: database}

	if _, err := store.Grant(context.Background(), "grant-1", "org-a", "cap.agent", ScopeKindAgent, "agent-a", "actor"); err == nil {
		t.Fatal("zero-row grant acknowledged")
	}
	database.err = errors.New("database unavailable")
	if _, err := store.Grant(context.Background(), "grant-1", "org-a", "cap.agent", ScopeKindAgent, "agent-a", "actor"); err == nil {
		t.Fatal("failed grant acknowledged")
	}
	if _, err := store.Revoke(context.Background(), "org-a", "cap.agent", ScopeKindAgent, "agent-a"); err == nil {
		t.Fatal("failed revoke acknowledged")
	}
	if _, err := store.IsGrantedForScope(context.Background(), "cap.agent", "org-a", ScopeKindAgent, "agent-a"); err == nil {
		t.Fatal("failed resolution allowed")
	}
	if _, err := store.HasAnyGrants(context.Background(), "cap.agent", "org-a", ScopeKindAgent); err == nil {
		t.Fatal("failed grant governance check allowed")
	}
	if _, err := store.ListForCapabilityForOrg(context.Background(), "cap.agent", "org-a"); err == nil {
		t.Fatal("failed tenant list allowed")
	}
	if _, err := store.ResolveForScopeForOrg(context.Background(), "org-a", ScopeKindAgent, "agent-a"); err == nil {
		t.Fatal("failed tenant resolve allowed")
	}
}

func TestScopeStoreHasAnyGrantsAndInputGuards(t *testing.T) {
	t.Parallel()
	database := &scopeDatabaseStub{boolValue: true}
	store := &ScopeStore{pool: database}

	governed, err := store.HasAnyGrants(context.Background(), "cap.agent", "org-a", ScopeKindAgent)
	if err != nil || !governed {
		t.Fatalf("HasAnyGrants = %v, err = %v", governed, err)
	}
	if len(database.args) != 3 || database.args[1] != "org-a" {
		t.Fatalf("tenant governance args = %#v", database.args)
	}
	if _, err := store.ListForCapabilityForOrg(context.Background(), "", "org-a"); err == nil {
		t.Fatal("empty capability list input accepted")
	}
	if _, err := store.ResolveForScopeForOrg(context.Background(), "", ScopeKindAgent, "agent-a"); err == nil {
		t.Fatal("empty tenant resolution accepted")
	}
	if granted, err := store.IsGrantedForScope(context.Background(), "cap.agent", "", ScopeKindAgent, "agent-a"); err != nil || granted {
		t.Fatalf("empty tenant grant = %v, err = %v", granted, err)
	}
}
