package workspace

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// workspaceDatabaseStub mirrors capability-core's scope_store_test.go's own
// scopeDatabaseStub / sandbox-manager's own lease_test.go's
// leaseDatabaseStub: a hand-rolled workspaceDatabase recording every call's
// query/args, with configurable Exec and Query results.
type workspaceDatabaseStub struct {
	execTag  pgconn.CommandTag
	execErr  error
	rows     pgx.Rows
	queryErr error

	queries []string
	args    [][]any
}

func (d *workspaceDatabaseStub) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	d.queries = append(d.queries, query)
	d.args = append(d.args, append([]any(nil), args...))
	return d.execTag, d.execErr
}

func (d *workspaceDatabaseStub) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	d.queries = append(d.queries, query)
	d.args = append(d.args, append([]any(nil), args...))
	return d.rows, d.queryErr
}

// manifestRows is a fake pgx.Rows over a fixed set of (path, content_hash)
// pairs, the same generic Values-based shape as capability-core's scopeRows.
type manifestRows struct {
	values [][]any
	index  int
}

func (r *manifestRows) Close()                                       {}
func (r *manifestRows) Err() error                                   { return nil }
func (r *manifestRows) CommandTag() pgconn.CommandTag                { return pgconn.CommandTag{} }
func (r *manifestRows) FieldDescriptions() []pgconn.FieldDescription { return nil }
func (r *manifestRows) RawValues() [][]byte                          { return nil }
func (r *manifestRows) Conn() *pgx.Conn                              { return nil }
func (r *manifestRows) Next() bool {
	r.index++
	return r.index < len(r.values)
}
func (r *manifestRows) Values() ([]any, error) {
	if r.index < 0 || r.index >= len(r.values) {
		return nil, pgx.ErrNoRows
	}
	return append([]any(nil), r.values[r.index]...), nil
}
func (r *manifestRows) Scan(dest ...any) error {
	values, err := r.Values()
	if err != nil {
		return err
	}
	for i, d := range dest {
		reflect.ValueOf(d).Elem().Set(reflect.ValueOf(values[i]))
	}
	return nil
}

func TestNewStoreRejectsNilPool(t *testing.T) {
	t.Parallel()
	if _, err := NewStore(nil); err == nil {
		t.Fatal("nil pool unexpectedly accepted")
	}
}

func TestGetManifestRejectsMissingIdentifiers(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{}
	store := &Store{pool: database}

	if _, err := store.GetManifest(context.Background(), "", "space-1", "run-1"); err == nil {
		t.Fatal("missing org_id unexpectedly accepted")
	}
	if _, err := store.GetManifest(context.Background(), "org-a", "", "run-1"); err == nil {
		t.Fatal("missing space_id unexpectedly accepted")
	}
	if _, err := store.GetManifest(context.Background(), "org-a", "space-1", ""); err == nil {
		t.Fatal("missing run_id unexpectedly accepted")
	}
	if len(database.queries) != 0 {
		t.Fatalf("no query should run for a rejected call, got %d", len(database.queries))
	}
}

func TestGetManifestReturnsTheLayeredView(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{rows: &manifestRows{index: -1, values: [][]any{
		{"a.txt", "sha256:overlay-a"},
		{"b.txt", "sha256:space-b"},
	}}}
	store := &Store{pool: database}

	entries, err := store.GetManifest(context.Background(), "org-a", "space-1", "lease-1")
	if err != nil {
		t.Fatalf("GetManifest: %v", err)
	}
	want := []ManifestEntry{
		{Path: "a.txt", ContentHash: "sha256:overlay-a"},
		{Path: "b.txt", ContentHash: "sha256:space-b"},
	}
	if !reflect.DeepEqual(entries, want) {
		t.Fatalf("entries = %+v, want %+v", entries, want)
	}

	if len(database.queries) != 1 {
		t.Fatalf("expected exactly one query, got %d", len(database.queries))
	}
	if !strings.Contains(database.queries[0], "DISTINCT ON (path)") {
		t.Fatalf("query does not layer overlay over space rows: %s", database.queries[0])
	}
	if !strings.Contains(database.queries[0], "run_id IS NULL OR run_id = $3") {
		t.Fatalf("query does not scope to this run's overlay: %s", database.queries[0])
	}
	wantArgs := []any{"org-a", "space-1", "lease-1"}
	if !reflect.DeepEqual(database.args[0], wantArgs) {
		t.Fatalf("args = %#v, want %#v", database.args[0], wantArgs)
	}
}

func TestUpsertOverlayRejectsMissingIdentifiers(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{}
	store := &Store{pool: database}

	err := store.UpsertOverlay(context.Background(), "", "space-1", "run-1", []ChangedFile{
		{Path: "a.txt", ContentHash: "sha256:a"},
	})
	if err == nil {
		t.Fatal("missing org_id unexpectedly accepted")
	}
	if len(database.queries) != 0 {
		t.Fatalf("no write should run for a rejected call, got %d", len(database.queries))
	}
}

func TestUpsertOverlayRejectsAFileMissingPathOrHash(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{}
	store := &Store{pool: database}

	err := store.UpsertOverlay(context.Background(), "org-a", "space-1", "run-1", []ChangedFile{
		{Path: "", ContentHash: "sha256:a"},
	})
	if err == nil {
		t.Fatal("empty path unexpectedly accepted")
	}
}

func TestUpsertOverlayWritesOnePerFileWithBaseHash(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{execTag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &Store{pool: database}

	files := []ChangedFile{
		{Path: "new.txt", ContentHash: "sha256:new", SizeBytes: 10, BaseHash: ""},
		{Path: "changed.txt", ContentHash: "sha256:changed", SizeBytes: 20, BaseHash: "sha256:old"},
	}
	if err := store.UpsertOverlay(context.Background(), "org-a", "space-1", "lease-1", files); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}
	if len(database.queries) != 2 {
		t.Fatalf("expected one write per file, got %d", len(database.queries))
	}
	for _, q := range database.queries {
		if !strings.Contains(q, "ON CONFLICT (org_id, space_id, COALESCE(run_id, ''), path)") {
			t.Fatalf("write does not target the migration's own identity index: %s", q)
		}
	}
	wantFirst := []any{"org-a", "space-1", "lease-1", "new.txt", "sha256:new", "", int64(10)}
	if !reflect.DeepEqual(database.args[0], wantFirst) {
		t.Fatalf("first write args = %#v, want %#v", database.args[0], wantFirst)
	}
	wantSecond := []any{"org-a", "space-1", "lease-1", "changed.txt", "sha256:changed", "sha256:old", int64(20)}
	if !reflect.DeepEqual(database.args[1], wantSecond) {
		t.Fatalf("second write args = %#v, want %#v", database.args[1], wantSecond)
	}
}

func TestUpsertOverlaySurfacesAWriteFailure(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{execErr: errors.New("connection reset")}
	store := &Store{pool: database}

	err := store.UpsertOverlay(context.Background(), "org-a", "space-1", "lease-1", []ChangedFile{
		{Path: "a.txt", ContentHash: "sha256:a", SizeBytes: 1},
	})
	if err == nil {
		t.Fatal("write failure unexpectedly swallowed")
	}
	if !strings.Contains(err.Error(), "a.txt") {
		t.Fatalf("error does not name the failing file: %v", err)
	}
}
