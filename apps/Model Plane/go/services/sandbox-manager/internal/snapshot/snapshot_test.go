package snapshot

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"

	"github.com/triodelab/model-plane/services/sandbox-manager/internal/lease"
)

// snapshotDatabaseStub mirrors lease's own leaseDatabaseStub (itself mirroring
// capability-core's scope_store_test.go pattern).
type snapshotDatabaseStub struct {
	execTag  pgconn.CommandTag
	execErr  error
	row      pgx.Row
	queries  []string
	argsList [][]any
}

func (d *snapshotDatabaseStub) Exec(_ context.Context, query string, args ...any) (pgconn.CommandTag, error) {
	d.queries = append(d.queries, query)
	d.argsList = append(d.argsList, append([]any(nil), args...))
	return d.execTag, d.execErr
}

func (d *snapshotDatabaseStub) Query(_ context.Context, query string, args ...any) (pgx.Rows, error) {
	return nil, errors.New("Query is not used by snapshot.Store")
}

func (d *snapshotDatabaseStub) QueryRow(_ context.Context, query string, args ...any) pgx.Row {
	return d.row
}

func (d *snapshotDatabaseStub) lastQuery() string {
	if len(d.queries) == 0 {
		return ""
	}
	return d.queries[len(d.queries)-1]
}

func (d *snapshotDatabaseStub) lastArgs() []any {
	if len(d.argsList) == 0 {
		return nil
	}
	return d.argsList[len(d.argsList)-1]
}

type snapshotRow struct {
	values []any
	err    error
}

func (r snapshotRow) Scan(dest ...any) error {
	if r.err != nil {
		return r.err
	}
	*dest[0].(*string) = r.values[0].(string)
	*dest[1].(*string) = r.values[1].(string)
	*dest[2].(*string) = r.values[2].(string)
	*dest[3].(*string) = r.values[3].(string)
	*dest[4].(*time.Time) = r.values[4].(time.Time)
	return nil
}

func fixedRand(buf []byte) (int, error) {
	for i := range buf {
		buf[i] = 0xCD
	}
	return len(buf), nil
}

func newTestStore(row pgx.Row) (*Store, *snapshotDatabaseStub) {
	database := &snapshotDatabaseStub{execTag: pgconn.NewCommandTag("INSERT 0 1"), row: row}
	return &Store{pool: database, nowFn: time.Now, randFn: fixedRand}, database
}

func TestNewStoreRejectsNilPool(t *testing.T) {
	if _, err := NewStore(nil); err == nil {
		t.Fatal("expected error for nil pool")
	}
}

func TestCreateRejectsAnInvalidLease(t *testing.T) {
	store, database := newTestStore(nil)
	if _, err := store.Create(context.Background(), nil, "label"); !errors.Is(err, ErrInvalidLease) {
		t.Fatalf("error = %v, want ErrInvalidLease", err)
	}
	if _, err := store.Create(context.Background(), &lease.Lease{}, "label"); !errors.Is(err, ErrInvalidLease) {
		t.Fatalf("error = %v, want ErrInvalidLease (empty lease id)", err)
	}
	if len(database.queries) != 0 {
		t.Fatalf("invalid lease reached the database: %v", database.queries)
	}
}

func TestCreateInsertsASnapshotBoundToTheLease(t *testing.T) {
	store, database := newTestStore(nil)
	l := &lease.Lease{ID: "lease-1"}
	snap, err := store.Create(context.Background(), l, "checkpoint-1")
	if err != nil {
		t.Fatal(err)
	}
	if snap.LeaseID != "lease-1" || snap.Label != "checkpoint-1" || snap.ID == "" {
		t.Fatalf("snapshot = %+v", snap)
	}
	if !strings.HasPrefix(snap.ObjectKey, "snapshots/lease-1/") {
		t.Fatalf("ObjectKey = %q, want a snapshots/lease-1/ prefix", snap.ObjectKey)
	}
	if !strings.Contains(database.lastQuery(), "INSERT INTO snapshots") {
		t.Fatalf("query = %q, want an INSERT into snapshots", database.lastQuery())
	}
	args := database.lastArgs()
	if args[1] != "lease-1" || args[2] != "checkpoint-1" {
		t.Fatalf("insert args = %#v", args)
	}
}

func TestGetReportsNotFoundOnNoRows(t *testing.T) {
	store, _ := newTestStore(snapshotRow{err: pgx.ErrNoRows})
	if _, err := store.Get(context.Background(), "missing"); !errors.Is(err, ErrSnapshotNotFound) {
		t.Fatalf("error = %v, want ErrSnapshotNotFound", err)
	}
}

func TestGetReturnsTheStoredSnapshot(t *testing.T) {
	created := time.Now().UTC().Truncate(time.Microsecond)
	store, _ := newTestStore(snapshotRow{values: []any{"snap-1", "lease-1", "checkpoint-1", "snapshots/lease-1/snap-1", created}})
	snap, err := store.Get(context.Background(), "snap-1")
	if err != nil {
		t.Fatal(err)
	}
	if snap.ID != "snap-1" || snap.LeaseID != "lease-1" || snap.Label != "checkpoint-1" || !snap.CreatedAt.Equal(created) {
		t.Fatalf("snapshot = %+v", snap)
	}
}
