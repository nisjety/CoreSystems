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

func TestUpsertOverlayUnTombstonesOnConflict(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{execTag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &Store{pool: database}
	if err := store.UpsertOverlay(context.Background(), "org-a", "space-1", "lease-1", []ChangedFile{
		{Path: "a.txt", ContentHash: "sha256:a", SizeBytes: 1},
	}); err != nil {
		t.Fatalf("UpsertOverlay: %v", err)
	}
	if !strings.Contains(database.queries[0], "deleted = false") {
		t.Fatalf("a recreated file does not clear the tombstone: %s", database.queries[0])
	}
}

func TestUpsertTombstonesRejectsMissingIdentifiersAndEmptyPath(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{}
	store := &Store{pool: database}
	d := []DeletedPath{{Path: "a.txt", BaseHash: "sha256:a"}}
	if err := store.UpsertTombstones(context.Background(), "", "space-1", "run-1", d); err == nil {
		t.Fatal("missing org_id unexpectedly accepted")
	}
	if err := store.UpsertTombstones(context.Background(), "org-a", "", "run-1", d); err == nil {
		t.Fatal("missing space_id unexpectedly accepted")
	}
	if err := store.UpsertTombstones(context.Background(), "org-a", "space-1", "", d); err == nil {
		t.Fatal("missing run_id unexpectedly accepted")
	}
	if err := store.UpsertTombstones(context.Background(), "org-a", "space-1", "run-1", []DeletedPath{{BaseHash: "sha256:a"}}); err == nil {
		t.Fatal("empty path unexpectedly accepted")
	}
	if len(database.queries) != 0 {
		t.Fatalf("no write should run for a rejected call, got %d", len(database.queries))
	}
}

func TestUpsertTombstonesWritesOneTombstoneRowPerPathWithTheBaseHash(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{execTag: pgconn.NewCommandTag("INSERT 0 1")}
	store := &Store{pool: database}

	err := store.UpsertTombstones(context.Background(), "org-a", "space-1", "lease-1", []DeletedPath{
		{Path: "gone.txt", BaseHash: "sha256:gone"},
		{Path: "made-and-removed.txt"},
	})
	if err != nil {
		t.Fatalf("UpsertTombstones: %v", err)
	}
	if len(database.queries) != 2 {
		t.Fatalf("expected one statement per path, got %d", len(database.queries))
	}
	tomb := database.queries[0]
	for _, want := range []string{
		"ON CONFLICT (org_id, space_id, COALESCE(run_id, ''), path)",
		"VALUES ($1, $2, $3, $4, $5, $5, 0, true, now())",
		"deleted = true",
	} {
		if !strings.Contains(tomb, want) {
			t.Fatalf("tombstone upsert is missing %q: %s", want, tomb)
		}
	}
	wantArgs := []any{"org-a", "space-1", "lease-1", "gone.txt", "sha256:gone"}
	if !reflect.DeepEqual(database.args[0], wantArgs) {
		t.Fatalf("tombstone args = %#v, want %#v", database.args[0], wantArgs)
	}
	// A path with no base hash never existed in the Space: the run's own live
	// overlay row is dropped and no tombstone (with an empty hash) is written.
	drop := database.queries[1]
	if !strings.Contains(drop, "DELETE FROM workspace_files") || !strings.Contains(drop, "NOT deleted") || strings.Contains(drop, "INSERT") {
		t.Fatalf("empty-base path did not drop the run's overlay row: %s", drop)
	}
	wantDropArgs := []any{"org-a", "space-1", "lease-1", "made-and-removed.txt"}
	if !reflect.DeepEqual(database.args[1], wantDropArgs) {
		t.Fatalf("drop args = %#v, want %#v", database.args[1], wantDropArgs)
	}
}

func TestUpsertTombstonesSurfacesAWriteFailure(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{execErr: errors.New("connection reset")}
	store := &Store{pool: database}
	err := store.UpsertTombstones(context.Background(), "org-a", "space-1", "lease-1", []DeletedPath{{Path: "a.txt", BaseHash: "sha256:a"}})
	if err == nil {
		t.Fatal("write failure unexpectedly swallowed")
	}
	if !strings.Contains(err.Error(), "a.txt") {
		t.Fatalf("error does not name the failing path: %v", err)
	}
}

// TestPromoteRunsOneCompareAndSwapDeletePerTombstone proves the Go-side wiring
// of tombstones in Promote: each tombstone is one statement that deletes the
// Space row only at the base hash, and its result is read as merged (a row
// removed), an idempotent replay (nothing removed, the Space row already gone)
// or a conflict (nothing removed, a Space row still there). The SQL itself is
// proven against real Postgres in store_integration_test.go.
func TestPromoteRunsOneCompareAndSwapDeletePerTombstone(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{
		queryRows: []pgx.Rows{
			&manifestRows{index: -1, values: [][]any{
				{"deleted.txt", true},
				{"replayed.txt", true},
				{"changed.txt", true},
				{"plain.txt", false},
			}},
			&manifestRows{index: -1, values: [][]any{{int64(1), true}}},  // deleted.txt: row removed
			&manifestRows{index: -1, values: [][]any{{int64(0), false}}}, // replayed.txt: already gone
			&manifestRows{index: -1, values: [][]any{{int64(0), true}}},  // changed.txt: hash moved
		},
		execTag: pgconn.NewCommandTag("INSERT 0 1"), // plain.txt merges
	}
	store := &Store{pool: database}

	conflicts, err := store.Promote(context.Background(), "org-a", "space-1", "lease-1")
	if err != nil {
		t.Fatalf("Promote: %v", err)
	}
	if !reflect.DeepEqual(conflicts, []string{"changed.txt"}) {
		t.Fatalf("conflicts = %v, want [changed.txt]", conflicts)
	}
	if len(database.queries) != 5 { // 1 listing + 3 tombstone statements + 1 upsert
		t.Fatalf("expected 1 listing + 3 tombstone statements + 1 upsert, got %d", len(database.queries))
	}
	for _, q := range database.queries[1:4] {
		for _, want := range []string{"WITH overlay AS", "DELETE FROM workspace_files", "run_id IS NULL", "content_hash = (SELECT base_hash FROM overlay)", "AND deleted"} {
			if !strings.Contains(q, want) {
				t.Fatalf("tombstone statement is missing %q: %s", want, q)
			}
		}
	}
	if want := []any{"org-a", "space-1", "lease-1", "changed.txt"}; !reflect.DeepEqual(database.args[3], want) {
		t.Fatalf("changed.txt tombstone args = %#v, want %#v", database.args[3], want)
	}
	if database.execCalls != 1 {
		t.Fatalf("expected exactly one plain upsert Exec, got %d", database.execCalls)
	}
}

func TestPromoteSurfacesATombstoneStatementFailure(t *testing.T) {
	t.Parallel()
	database := &workspaceDatabaseStub{
		queryRows: []pgx.Rows{&manifestRows{index: -1, values: [][]any{{"deleted.txt", true}}}},
	}
	store := &Store{pool: &failSecondQuery{workspaceDatabaseStub: database}}
	if _, err := store.Promote(context.Background(), "org-a", "space-1", "lease-1"); err == nil {
		t.Fatal("tombstone statement failure unexpectedly swallowed")
	}
}

// failSecondQuery fails every Query after the first, which is the listing.
type failSecondQuery struct {
	*workspaceDatabaseStub
	calls int
}

func (f *failSecondQuery) Query(ctx context.Context, query string, args ...any) (pgx.Rows, error) {
	f.calls++
	if f.calls > 1 {
		return nil, errors.New("connection reset")
	}
	return f.workspaceDatabaseStub.Query(ctx, query, args...)
}
