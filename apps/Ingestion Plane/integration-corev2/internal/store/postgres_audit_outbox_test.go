package store

import (
	"errors"
	"testing"
	"time"

	"github.com/pashagolub/pgxmock/v4"
)

func TestPostgresAuditOutboxClaimDecodesDurableEvent(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	createdAt := time.Now().UTC()
	mock.ExpectQuery(`FROM claim_integration_audit_event`).WillReturnRows(
		pgxmock.NewRows([]string{
			"id", "organization_id", "user_id", "connection_id", "event_type",
			"provider_key", "metadata", "request_id", "created_at", "attempts",
		}).AddRow(
			"audit-claim", "org-1", "user-1", "conn-1", "connection.created",
			"github", []byte(`{"safe":true}`), "req-1", createdAt, 2,
		),
	)

	event, found, err := repo.ClaimAuditEvent(t.Context())
	if err != nil || !found {
		t.Fatalf("ClaimAuditEvent found=%v err=%v", found, err)
	}
	if event.ID != "audit-claim" || event.Attempts != 2 || event.Metadata["safe"] != true {
		t.Fatalf("claimed event = %#v", event)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresInsertAuditEventAppliesDefaultsAndRejectsUnsafeMetadata(t *testing.T) {
	t.Run("defaults", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		mock.ExpectExec(`INSERT INTO integration_audit_events`).WithArgs(
			pgxmock.AnyArg(), "org-1", "", "", "test", "", pgxmock.AnyArg(), "", pgxmock.AnyArg(),
		).WillReturnResult(pgxmock.NewResult("INSERT", 1))
		if err := repo.InsertAuditEvent(t.Context(), AuditEvent{OrganizationID: "org-1", EventType: "test"}); err != nil {
			t.Fatalf("InsertAuditEvent: %v", err)
		}
	})

	t.Run("metadata marshal", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		err = repo.InsertAuditEvent(t.Context(), AuditEvent{
			ID: "audit-invalid-metadata", OrganizationID: "org-1", EventType: "test",
			Metadata: map[string]any{"invalid": make(chan struct{})}, CreatedAt: time.Now().UTC(),
		})
		if err == nil {
			t.Fatal("unsafe metadata error = nil")
		}
	})
}

func TestPostgresAuditOutboxClaimHandlesEmptyAndInvalidRows(t *testing.T) {
	t.Run("empty", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		mock.ExpectQuery(`FROM claim_integration_audit_event`).WillReturnRows(
			pgxmock.NewRows([]string{
				"id", "organization_id", "user_id", "connection_id", "event_type",
				"provider_key", "metadata", "request_id", "created_at", "attempts",
			}),
		)
		if _, found, err := repo.ClaimAuditEvent(t.Context()); err != nil || found {
			t.Fatalf("ClaimAuditEvent found=%v err=%v", found, err)
		}
	})

	t.Run("invalid metadata", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		mock.ExpectQuery(`FROM claim_integration_audit_event`).WillReturnRows(
			pgxmock.NewRows([]string{
				"id", "organization_id", "user_id", "connection_id", "event_type",
				"provider_key", "metadata", "request_id", "created_at", "attempts",
			}).AddRow("audit-invalid", "org-1", "", "", "test", "", []byte(`{`), "", time.Now().UTC(), 1),
		)
		if _, _, err := repo.ClaimAuditEvent(t.Context()); err == nil {
			t.Fatal("invalid metadata error = nil")
		}
	})
}

func TestPostgresAuditOutboxCompletionAndRetryAreLeaseBound(t *testing.T) {
	t.Run("complete", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		mock.ExpectExec(`SET published_at = now\(\)`).WithArgs("audit-1", 3).
			WillReturnResult(pgxmock.NewResult("UPDATE", 1))
		if err := repo.CompleteAuditEvent(t.Context(), "audit-1", 3); err != nil {
			t.Fatalf("CompleteAuditEvent: %v", err)
		}
	})

	t.Run("complete lease lost", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		mock.ExpectExec(`SET published_at = now\(\)`).WithArgs("audit-1", 2).
			WillReturnResult(pgxmock.NewResult("UPDATE", 0))
		if err := repo.CompleteAuditEvent(t.Context(), "audit-1", 2); err == nil {
			t.Fatal("lease-lost completion error = nil")
		}
	})

	t.Run("retry", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		nextAttempt := time.Now().UTC().Add(time.Minute)
		mock.ExpectExec(`SET processing_at = NULL`).WithArgs("audit-1", 4, nextAttempt, "audit unavailable", true).
			WillReturnResult(pgxmock.NewResult("UPDATE", 1))
		if err := repo.FailAuditEvent(t.Context(), "audit-1", 4, nextAttempt, "audit unavailable", true); err != nil {
			t.Fatalf("FailAuditEvent: %v", err)
		}
	})

	t.Run("retry database error", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("new pgx mock pool: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		wantErr := errors.New("database unavailable")
		nextAttempt := time.Now().UTC().Add(time.Minute)
		mock.ExpectExec(`SET processing_at = NULL`).WithArgs("audit-1", 4, nextAttempt, "audit unavailable", false).WillReturnError(wantErr)
		if err := repo.FailAuditEvent(t.Context(), "audit-1", 4, nextAttempt, "audit unavailable", false); !errors.Is(err, wantErr) {
			t.Fatalf("FailAuditEvent error = %v, want %v", err, wantErr)
		}
	})
}

func TestPostgresAuditOutboxStatsAndTerminalRecovery(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	mock.ExpectQuery(`COUNT\(\*\) FILTER`).WillReturnRows(
		pgxmock.NewRows([]string{"pending", "terminal", "pending_age", "terminal_age"}).
			AddRow(3, 2, 90.0, 300.0),
	)
	stats, err := repo.AuditOutboxStats(t.Context())
	if err != nil {
		t.Fatalf("AuditOutboxStats error: %v", err)
	}
	if stats.Pending != 3 || stats.Terminal != 2 || stats.OldestPendingAge != 90*time.Second || stats.OldestTerminalAge != 5*time.Minute {
		t.Fatalf("AuditOutboxStats = %#v", stats)
	}
	mock.ExpectQuery(`requeue_terminal_integration_audit_events`).
		WithArgs([]string{"audit-1", "audit-2"}).
		WillReturnRows(pgxmock.NewRows([]string{"requeued"}).AddRow(2))
	if requeued, err := repo.RequeueTerminalAuditEvents(t.Context(), []string{"audit-1", "audit-2"}); err != nil || requeued != 2 {
		t.Fatalf("RequeueTerminalAuditEvents = (%d, %v), want (2, nil)", requeued, err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}
