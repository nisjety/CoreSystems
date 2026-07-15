package store

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/pashagolub/pgxmock/v4"
)

func TestMemoryAuditTransactionRollsBackMutationWhenAuditFails(t *testing.T) {
	repo := NewMemoryRepository()
	_, err := repo.UpsertConnection(t.Context(), Connection{
		ID: "conn-rollback", OrganizationID: "org-1", UserID: "user-1", Status: "active",
	})
	if err != nil {
		t.Fatalf("seed connection: %v", err)
	}

	wantErr := errors.New("audit insert unavailable")
	err = repo.WithAuditTransaction(t.Context(), func(tx AuditTransaction) error {
		if _, updateErr := tx.UpdateConnectionCapabilities(t.Context(), "conn-rollback", []string{"mail.send"}); updateErr != nil {
			return updateErr
		}
		return wantErr
	})
	if !errors.Is(err, wantErr) {
		t.Fatalf("WithAuditTransaction error = %v, want %v", err, wantErr)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-rollback")
	if err != nil {
		t.Fatalf("get connection: %v", err)
	}
	if len(connection.Capabilities) != 0 {
		t.Fatalf("capabilities committed without audit: %#v", connection.Capabilities)
	}
}

func TestMemoryAuditTransactionCommitsMutationAndIntentTogether(t *testing.T) {
	repo := NewMemoryRepository()
	_, err := repo.UpsertConnection(t.Context(), Connection{
		ID: "conn-commit", OrganizationID: "org-1", UserID: "user-1", Status: "active",
	})
	if err != nil {
		t.Fatalf("seed connection: %v", err)
	}
	event := AuditEvent{
		ID: "audit:integration:capabilities:conn-commit", OrganizationID: "org-1",
		ConnectionID: "conn-commit", EventType: "connection.capabilities.updated", CreatedAt: time.Now().UTC(),
	}
	err = repo.WithAuditTransaction(t.Context(), func(tx AuditTransaction) error {
		if _, updateErr := tx.UpdateConnectionCapabilities(t.Context(), "conn-commit", []string{"mail.send"}); updateErr != nil {
			return updateErr
		}
		return tx.InsertAuditEvent(t.Context(), event)
	})
	if err != nil {
		t.Fatalf("WithAuditTransaction: %v", err)
	}
	connection, err := repo.GetConnection(t.Context(), "conn-commit")
	if err != nil {
		t.Fatalf("get connection: %v", err)
	}
	if len(connection.Capabilities) != 1 || connection.Capabilities[0] != "mail.send" {
		t.Fatalf("capabilities = %#v", connection.Capabilities)
	}
	claimed, found, err := repo.ClaimAuditEvent(t.Context())
	if err != nil || !found {
		t.Fatalf("claim audit event: found=%v err=%v", found, err)
	}
	if claimed.ID != event.ID {
		t.Fatalf("audit event = %q, want %q", claimed.ID, event.ID)
	}
	if err := repo.FailAuditEvent(t.Context(), claimed.ID, claimed.Attempts, time.Now().UTC().Add(-time.Second), "retry", false); err != nil {
		t.Fatalf("FailAuditEvent: %v", err)
	}
	claimedAgain, found, err := repo.ClaimAuditEvent(t.Context())
	if err != nil || !found {
		t.Fatalf("reclaim audit event: found=%v err=%v", found, err)
	}
	if claimedAgain.Attempts != claimed.Attempts+1 {
		t.Fatalf("reclaimed attempts = %d, want %d", claimedAgain.Attempts, claimed.Attempts+1)
	}
	if err := repo.CompleteAuditEvent(t.Context(), claimedAgain.ID, claimedAgain.Attempts); err != nil {
		t.Fatalf("CompleteAuditEvent: %v", err)
	}
	if err := repo.CompleteAuditEvent(t.Context(), claimedAgain.ID, claimed.Attempts); !errors.Is(err, ErrConflict) {
		t.Fatalf("stale CompleteAuditEvent error = %v, want ErrConflict", err)
	}
	if _, found, err := repo.ClaimAuditEvent(t.Context()); err != nil || found {
		t.Fatalf("published event reclaimed: found=%v err=%v", found, err)
	}

	terminal := AuditEvent{
		ID: "audit:integration:terminal", OrganizationID: "org-1", EventType: "test", CreatedAt: time.Now().UTC(),
	}
	if err := repo.InsertAuditEvent(t.Context(), terminal); err != nil {
		t.Fatalf("InsertAuditEvent terminal: %v", err)
	}
	claimedTerminal, found, err := repo.ClaimAuditEvent(t.Context())
	if err != nil || !found {
		t.Fatalf("claim terminal candidate: found=%v err=%v", found, err)
	}
	if err := repo.FailAuditEvent(t.Context(), claimedTerminal.ID, claimedTerminal.Attempts, time.Now().UTC(), "poison", true); err != nil {
		t.Fatalf("terminal FailAuditEvent: %v", err)
	}
	if _, found, err := repo.ClaimAuditEvent(t.Context()); err != nil || found {
		t.Fatalf("terminal event reclaimed: found=%v err=%v", found, err)
	}
	stats, err := repo.AuditOutboxStats(t.Context())
	if err != nil || stats.Terminal != 1 {
		t.Fatalf("terminal stats = %#v error=%v, want one", stats, err)
	}
	if requeued, err := repo.RequeueTerminalAuditEvents(t.Context(), []string{terminal.ID}); err != nil || requeued != 1 {
		t.Fatalf("terminal requeue = (%d, %v), want (1, nil)", requeued, err)
	}
	requeuedEvent, found, err := repo.ClaimAuditEvent(t.Context())
	if err != nil || !found || requeuedEvent.ID != terminal.ID || requeuedEvent.Attempts != 1 {
		t.Fatalf("requeued claim = (%#v, %v, %v), want reset attempt", requeuedEvent, found, err)
	}
}

func TestPostgresAuditTransactionRollsBackWhenIntentInsertFails(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	wantErr := errors.New("audit insert unavailable")

	mock.ExpectBegin()
	mock.ExpectExec(`INSERT INTO integration_audit_events`).
		WithArgs(
			"audit:integration:test", "org-1", "", "", "test", "",
			pgxmock.AnyArg(), "", pgxmock.AnyArg(),
		).
		WillReturnError(wantErr)
	mock.ExpectRollback()
	err = repo.WithAuditTransaction(context.Background(), func(tx AuditTransaction) error {
		return tx.InsertAuditEvent(context.Background(), AuditEvent{
			ID: "audit:integration:test", OrganizationID: "org-1", EventType: "test", CreatedAt: time.Now().UTC(),
		})
	})
	if !errors.Is(err, wantErr) {
		t.Fatalf("WithAuditTransaction error = %v, want %v", err, wantErr)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresAuditTransactionCommitsMutationAndIntent(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}

	mock.ExpectBegin()
	mock.ExpectExec(`INSERT INTO integration_audit_events`).
		WithArgs(
			"audit:integration:commit", "org-1", "", "", "test", "",
			pgxmock.AnyArg(), "", pgxmock.AnyArg(),
		).
		WillReturnResult(pgxmock.NewResult("INSERT", 1))
	mock.ExpectCommit()
	err = repo.WithAuditTransaction(t.Context(), func(tx AuditTransaction) error {
		return tx.InsertAuditEvent(t.Context(), AuditEvent{
			ID: "audit:integration:commit", OrganizationID: "org-1", EventType: "test", CreatedAt: time.Now().UTC(),
		})
	})
	if err != nil {
		t.Fatalf("WithAuditTransaction: %v", err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresAuditTransactionReportsLifecycleFailures(t *testing.T) {
	t.Run("nil callback", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		if err := repo.WithAuditTransaction(t.Context(), nil); err == nil {
			t.Fatal("nil callback error = nil")
		}
	})

	t.Run("begin", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		wantErr := errors.New("begin unavailable")
		mock.ExpectBegin().WillReturnError(wantErr)
		if err := repo.WithAuditTransaction(t.Context(), func(AuditTransaction) error { return nil }); !errors.Is(err, wantErr) {
			t.Fatalf("begin error = %v, want %v", err, wantErr)
		}
	})

	t.Run("commit", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		wantErr := errors.New("commit unavailable")
		mock.ExpectBegin()
		mock.ExpectCommit().WillReturnError(wantErr)
		if err := repo.WithAuditTransaction(t.Context(), func(AuditTransaction) error { return nil }); !errors.Is(err, wantErr) {
			t.Fatalf("commit error = %v, want %v", err, wantErr)
		}
	})
}
