package procedures

import (
	"context"
	"errors"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// int4Max is the largest value of the INTEGER version columns (migration 0043).
const int4Max = 2147483647

// beyondInt4 is one more than the largest INTEGER value. A version number this large cannot be stored.
const beyondInt4 = 2147483648

// testProcedureID has the shape ValidID accepts; the store never reaches a database for an id it refuses.
const testProcedureID = "proc_11111111-1111-1111-1111-111111111111"

// errDBReached is what unreachableDB answers. A test that sees it knows the store asked the database.
var errDBReached = errors.New("the database was reached")

// unreachableDB answers every call with errDBReached and counts the calls.
type unreachableDB struct{ calls int }

func (d *unreachableDB) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	d.calls++
	return pgconn.CommandTag{}, errDBReached
}

func (d *unreachableDB) Query(context.Context, string, ...any) (pgx.Rows, error) {
	d.calls++
	return nil, errDBReached
}

func (d *unreachableDB) QueryRow(context.Context, string, ...any) pgx.Row {
	d.calls++
	return failedRow{errDBReached}
}

func (d *unreachableDB) Begin(context.Context) (pgx.Tx, error) {
	d.calls++
	return nil, errDBReached
}

// failedRow is a pgx.Row whose Scan answers its error.
type failedRow struct{ err error }

func (r failedRow) Scan(...any) error { return r.err }

// A version number above the INTEGER range names no stored version. It is answered ErrNotFound before any
// statement is sent: pgx cannot encode it as the int4 parameter, and the caller would see a store failure.
func TestVersionBeyondTheIntegerColumnIsNotFound(t *testing.T) {
	ctx := context.Background()
	who := Owner{OrgID: "org_1", UserID: "user_1"}
	for _, version := range []int{beyondInt4, 3000000000} {
		db := &unreachableDB{}
		store, err := NewStore(db)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := store.GetVersion(ctx, who, testProcedureID, Ref{Version: version}); !errors.Is(err, ErrNotFound) {
			t.Fatalf("GetVersion(%d) = %v, want ErrNotFound", version, err)
		}
		if err := store.SetTag(ctx, who, testProcedureID, "stable", version); !errors.Is(err, ErrNotFound) {
			t.Fatalf("SetTag(%d) = %v, want ErrNotFound", version, err)
		}
		if db.calls != 0 {
			t.Fatalf("version %d reached the database %d times, want 0", version, db.calls)
		}
	}
}

// The largest number the column holds is still a version that may exist, so it is asked of the database.
func TestVersionAtTheIntegerColumnLimitReachesTheDatabase(t *testing.T) {
	ctx := context.Background()
	who := Owner{OrgID: "org_1", UserID: "user_1"}
	db := &unreachableDB{}
	store, err := NewStore(db)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := store.GetVersion(ctx, who, testProcedureID, Ref{Version: int4Max}); !errors.Is(err, errDBReached) {
		t.Fatalf("GetVersion(int4 max) = %v, want the database to be asked", err)
	}
	if err := store.SetTag(ctx, who, testProcedureID, "stable", int4Max); !errors.Is(err, errDBReached) {
		t.Fatalf("SetTag(int4 max) = %v, want the database to be asked", err)
	}
	if db.calls != 2 {
		t.Fatalf("database calls = %d, want 2", db.calls)
	}
}
