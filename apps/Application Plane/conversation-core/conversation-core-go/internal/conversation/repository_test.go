package conversation

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// fakeTx implements pgx.Tx for transactional unit tests. Only Exec/Commit/Rollback
// are exercised by ReviewAIAction; the remaining pgx.Tx methods are inherited from
// the embedded (nil) interface and panic if ever called — proving they are not.
type fakeTx struct {
	pgx.Tx
	execResults []pgconn.CommandTag
	execSQL     []string
	execCalls   int
	committed   bool
	rolledBack  bool
}

func (t *fakeTx) Exec(_ context.Context, sql string, _ ...any) (pgconn.CommandTag, error) {
	i := t.execCalls
	t.execCalls++
	t.execSQL = append(t.execSQL, sql)
	var tag pgconn.CommandTag
	if i < len(t.execResults) {
		tag = t.execResults[i]
	}
	return tag, nil
}

func (t *fakeTx) Commit(_ context.Context) error   { t.committed = true; return nil }
func (t *fakeTx) Rollback(_ context.Context) error { t.rolledBack = true; return nil }

// fakePool implements PgxPool. Only Begin is exercised by ReviewAIAction; the
// other methods panic to prove they are not reached on this path.
type fakePool struct {
	tx *fakeTx
}

func (p *fakePool) Begin(_ context.Context) (pgx.Tx, error)                 { return p.tx, nil }
func (p *fakePool) Query(context.Context, string, ...any) (pgx.Rows, error) { panic("unused") }
func (p *fakePool) QueryRow(context.Context, string, ...any) pgx.Row        { panic("unused") }
func (p *fakePool) Exec(context.Context, string, ...any) (pgconn.CommandTag, error) {
	panic("unused")
}

// A review for an action that does not exist for the org (missing or foreign-org
// id) must return ErrNotFound (→404) and must NOT insert a conversation_ai_reviews
// row — the UPDATE matched zero rows, so the transaction is rolled back before the
// INSERT ever runs.
func TestReviewAIActionReturnsNotFoundWithoutPhantomReview(t *testing.T) {
	tx := &fakeTx{execResults: []pgconn.CommandTag{pgconn.NewCommandTag("UPDATE 0")}}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	err := repo.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "missing",
		ReviewerID: "user_1",
		Decision:   "approved",
		OccurredAt: time.Date(2026, time.June, 19, 8, 0, 0, 0, time.UTC),
	})

	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("error = %v, want ErrNotFound", err)
	}
	if tx.execCalls != 1 {
		t.Fatalf("Exec called %d times, want 1 (UPDATE only; the INSERT must be skipped so no phantom review row is written)", tx.execCalls)
	}
	if tx.committed {
		t.Fatal("transaction committed; want rollback for a non-existent action")
	}
	if !tx.rolledBack {
		t.Fatal("transaction not rolled back for a non-existent action")
	}
}

// A review for an existing action runs the UPDATE then the review-row INSERT and
// commits.
func TestReviewAIActionCommitsWhenActionExists(t *testing.T) {
	tx := &fakeTx{execResults: []pgconn.CommandTag{
		pgconn.NewCommandTag("UPDATE 1"),
		pgconn.NewCommandTag("INSERT 0 1"),
	}}
	repo := &PGRepository{pool: &fakePool{tx: tx}}

	err := repo.ReviewAIAction(context.Background(), AIActionReview{
		OrgID:      "org_1",
		AIActionID: "aiact_1",
		ReviewerID: "user_1",
		Decision:   "approved",
		OccurredAt: time.Date(2026, time.June, 19, 8, 0, 0, 0, time.UTC),
	})

	if err != nil {
		t.Fatalf("error = %v, want nil", err)
	}
	if tx.execCalls != 2 {
		t.Fatalf("Exec called %d times, want 2 (UPDATE action + INSERT review row)", tx.execCalls)
	}
	if !tx.committed {
		t.Fatal("transaction not committed for an existing action")
	}
}
