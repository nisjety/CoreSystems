package store

import (
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/pashagolub/pgxmock/v4"
)

var actionReceiptTestColumns = []string{
	"organization_id", "idempotency_key", "request_sha256", "connection_id",
	"provider_key", "operation", "attestation_issuer", "attestation_kid", "authorization_kind",
	"authorization_id", "approval_id", "action_id", "actor_id", "attestation_jti", "payload_sha256",
	"status", "provider_message_id", "created_at", "updated_at",
}

func TestPostgresClaimActionReceiptInsertsExecutingReceipt(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	want := actionReceiptFixture()
	now := time.Now().UTC()

	mock.ExpectQuery(`INSERT INTO integration_action_receipts`).
		WithArgs(actionReceiptInsertArgs(want)...).
		WillReturnRows(pgxmock.NewRows(actionReceiptTestColumns).AddRow(actionReceiptRow(want, "pending", "", now)...))

	got, acquired, err := repo.ClaimActionReceipt(t.Context(), want)
	if err != nil {
		t.Fatalf("ClaimActionReceipt error: %v", err)
	}
	if !acquired || got.Status != "pending" {
		t.Fatalf("ClaimActionReceipt = (%#v, acquired=%v), want pending acquired receipt", got, acquired)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresClaimActionReceiptReturnsExistingReceiptAfterConflict(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	want := actionReceiptFixture()
	now := time.Now().UTC()

	mock.ExpectQuery(`INSERT INTO integration_action_receipts`).
		WithArgs(actionReceiptInsertArgs(want)...).
		WillReturnError(pgx.ErrNoRows)
	mock.ExpectQuery(`SELECT organization_id, idempotency_key, request_sha256, connection_id`).
		WithArgs(want.OrganizationID, want.IdempotencyKey, want.AttestationIssuer, want.AuthorizationID).
		WillReturnRows(pgxmock.NewRows(actionReceiptTestColumns).AddRow(actionReceiptRow(want, "completed", "provider-message-1", now)...))

	got, acquired, err := repo.ClaimActionReceipt(t.Context(), want)
	if err != nil {
		t.Fatalf("ClaimActionReceipt error: %v", err)
	}
	if acquired || got.Status != "completed" || got.ProviderMessageID != "provider-message-1" {
		t.Fatalf("ClaimActionReceipt = (%#v, acquired=%v), want stored completed receipt", got, acquired)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresClaimActionReceiptConflictsWhenAuthorizationUsesSecondKey(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	want := actionReceiptFixture()
	existing := want
	existing.IdempotencyKey = "conversation:org-1:reply-original"
	now := time.Now().UTC()

	mock.ExpectQuery(`INSERT INTO integration_action_receipts`).WithArgs(actionReceiptInsertArgs(want)...).WillReturnError(pgx.ErrNoRows)
	mock.ExpectQuery(`SELECT organization_id, idempotency_key, request_sha256, connection_id`).
		WithArgs(want.OrganizationID, want.IdempotencyKey, want.AttestationIssuer, want.AuthorizationID).
		WillReturnRows(pgxmock.NewRows(actionReceiptTestColumns).AddRow(actionReceiptRow(existing, "completed", "provider-message-1", now)...))
	if _, _, err := repo.ClaimActionReceipt(t.Context(), want); !errors.Is(err, ErrConflict) {
		t.Fatalf("ClaimActionReceipt error = %v, want ErrConflict", err)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresClaimActionReceiptAllowsFreshJTIAndRotatedTrustedKID(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	firstSeen := actionReceiptFixture()
	fresh := firstSeen
	fresh.AttestationKeyID = "conversation-write-rotated"
	fresh.AttestationJTI = "fresh-attestation-jti"
	now := time.Now().UTC()

	mock.ExpectQuery(`INSERT INTO integration_action_receipts`).WithArgs(actionReceiptInsertArgs(fresh)...).WillReturnError(pgx.ErrNoRows)
	mock.ExpectQuery(`SELECT organization_id, idempotency_key, request_sha256, connection_id`).
		WithArgs(fresh.OrganizationID, fresh.IdempotencyKey, fresh.AttestationIssuer, fresh.AuthorizationID).
		WillReturnRows(pgxmock.NewRows(actionReceiptTestColumns).AddRow(actionReceiptRow(firstSeen, "pending", "", now)...))
	got, acquired, err := repo.ClaimActionReceipt(t.Context(), fresh)
	if err != nil || acquired || got.AttestationKeyID != firstSeen.AttestationKeyID || got.AttestationJTI != firstSeen.AttestationJTI {
		t.Fatalf("ClaimActionReceipt = (%#v, acquired=%v, err=%v), want first-seen pending receipt", got, acquired, err)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresBeginActionReceiptExecutionRequiresPendingState(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	want := actionReceiptFixture()
	now := time.Now().UTC()

	mock.ExpectQuery(`UPDATE integration_action_receipts SET status = 'executing'.*status = 'pending'`).
		WithArgs(want.OrganizationID, want.IdempotencyKey).
		WillReturnRows(pgxmock.NewRows(actionReceiptTestColumns).AddRow(actionReceiptRow(want, "executing", "", now)...))
	got, err := repo.BeginActionReceiptExecution(t.Context(), want.OrganizationID, want.IdempotencyKey)
	if err != nil || got.Status != "executing" {
		t.Fatalf("BeginActionReceiptExecution = (%#v, %v), want executing", got, err)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresBeginActionReceiptExecutionReportsConflictAndStorageError(t *testing.T) {
	t.Run("non-pending receipt", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("pgxmock.NewPool error: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		want := actionReceiptFixture()
		mock.ExpectQuery(`UPDATE integration_action_receipts SET status = 'executing'.*status = 'pending'`).
			WithArgs(want.OrganizationID, want.IdempotencyKey).WillReturnError(pgx.ErrNoRows)
		mock.ExpectQuery(`SELECT status FROM integration_action_receipts`).WithArgs(want.OrganizationID, want.IdempotencyKey).
			WillReturnRows(pgxmock.NewRows([]string{"status"}).AddRow("executing"))
		if _, err := repo.BeginActionReceiptExecution(t.Context(), want.OrganizationID, want.IdempotencyKey); !errors.Is(err, ErrConflict) {
			t.Fatalf("BeginActionReceiptExecution error = %v, want ErrConflict", err)
		}
		assertPostgresExpectations(t, mock)
	})

	t.Run("storage failure", func(t *testing.T) {
		mock, err := pgxmock.NewPool()
		if err != nil {
			t.Fatalf("pgxmock.NewPool error: %v", err)
		}
		defer mock.Close()
		repo := &PostgresRepository{pool: mock}
		want := actionReceiptFixture()
		mock.ExpectQuery(`UPDATE integration_action_receipts SET status = 'executing'.*status = 'pending'`).
			WithArgs(want.OrganizationID, want.IdempotencyKey).WillReturnError(errors.New("database unavailable"))
		if _, err := repo.BeginActionReceiptExecution(t.Context(), want.OrganizationID, want.IdempotencyKey); err == nil || !strings.Contains(err.Error(), "begin action receipt execution") {
			t.Fatalf("BeginActionReceiptExecution error = %v, want wrapped storage failure", err)
		}
		assertPostgresExpectations(t, mock)
	})
}

func TestPostgresCompleteActionReceiptRequiresExecutingState(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	want := actionReceiptFixture()
	now := time.Now().UTC()

	mock.ExpectQuery(`UPDATE integration_action_receipts SET status = 'completed'.*status = 'executing'`).
		WithArgs(want.OrganizationID, want.IdempotencyKey, "provider-message-1").
		WillReturnRows(pgxmock.NewRows(actionReceiptTestColumns).AddRow(actionReceiptRow(want, "completed", "provider-message-1", now)...))

	got, err := repo.CompleteActionReceipt(t.Context(), want.OrganizationID, want.IdempotencyKey, " provider-message-1 ")
	if err != nil {
		t.Fatalf("CompleteActionReceipt error: %v", err)
	}
	if got.Status != "completed" || got.ProviderMessageID != "provider-message-1" {
		t.Fatalf("CompleteActionReceipt = %#v, want completed receipt", got)
	}

	mock.ExpectQuery(`UPDATE integration_action_receipts SET status = 'completed'.*status = 'executing'`).
		WithArgs(want.OrganizationID, want.IdempotencyKey, "provider-message-1").
		WillReturnError(pgx.ErrNoRows)
	mock.ExpectQuery(`SELECT status FROM integration_action_receipts`).
		WithArgs(want.OrganizationID, want.IdempotencyKey).
		WillReturnRows(pgxmock.NewRows([]string{"status"}).AddRow("completed"))
	if _, err := repo.CompleteActionReceipt(t.Context(), want.OrganizationID, want.IdempotencyKey, "provider-message-1"); !errors.Is(err, ErrConflict) {
		t.Fatalf("CompleteActionReceipt(non-executing) error = %v, want ErrConflict", err)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresCompleteActionReceiptMissingReturnsNotFound(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}

	mock.ExpectQuery(`UPDATE integration_action_receipts SET status = 'completed'.*status = 'executing'`).
		WithArgs("org-1", "missing-key", "provider-message-1").
		WillReturnError(pgx.ErrNoRows)
	mock.ExpectQuery(`SELECT status FROM integration_action_receipts`).
		WithArgs("org-1", "missing-key").
		WillReturnError(pgx.ErrNoRows)
	if _, err := repo.CompleteActionReceipt(t.Context(), "org-1", "missing-key", "provider-message-1"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("CompleteActionReceipt(missing) error = %v, want ErrNotFound", err)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresMarkActionReceiptUnknownRequiresExecutingState(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}
	want := actionReceiptFixture()

	mock.ExpectExec(`UPDATE integration_action_receipts SET status = 'unknown'.*status = 'executing'`).
		WithArgs(want.OrganizationID, want.IdempotencyKey).
		WillReturnResult(pgxmock.NewResult("UPDATE", 1))
	if err := repo.MarkActionReceiptUnknown(t.Context(), want.OrganizationID, want.IdempotencyKey); err != nil {
		t.Fatalf("MarkActionReceiptUnknown error: %v", err)
	}

	mock.ExpectExec(`UPDATE integration_action_receipts SET status = 'unknown'.*status = 'executing'`).
		WithArgs(want.OrganizationID, want.IdempotencyKey).
		WillReturnResult(pgxmock.NewResult("UPDATE", 0))
	mock.ExpectQuery(`SELECT status FROM integration_action_receipts`).
		WithArgs(want.OrganizationID, want.IdempotencyKey).
		WillReturnRows(pgxmock.NewRows([]string{"status"}).AddRow("unknown"))
	if err := repo.MarkActionReceiptUnknown(t.Context(), want.OrganizationID, want.IdempotencyKey); !errors.Is(err, ErrConflict) {
		t.Fatalf("MarkActionReceiptUnknown(non-executing) error = %v, want ErrConflict", err)
	}
	assertPostgresExpectations(t, mock)
}

func TestPostgresMarkActionReceiptUnknownMissingReturnsNotFound(t *testing.T) {
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("pgxmock.NewPool error: %v", err)
	}
	defer mock.Close()
	repo := &PostgresRepository{pool: mock}

	mock.ExpectExec(`UPDATE integration_action_receipts SET status = 'unknown'.*status = 'executing'`).
		WithArgs("org-1", "missing-key").
		WillReturnResult(pgxmock.NewResult("UPDATE", 0))
	mock.ExpectQuery(`SELECT status FROM integration_action_receipts`).
		WithArgs("org-1", "missing-key").
		WillReturnError(pgx.ErrNoRows)
	if err := repo.MarkActionReceiptUnknown(t.Context(), "org-1", "missing-key"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("MarkActionReceiptUnknown(missing) error = %v, want ErrNotFound", err)
	}
	assertPostgresExpectations(t, mock)
}

func assertPostgresExpectations(t *testing.T, mock pgxmock.PgxPoolIface) {
	t.Helper()
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("PostgreSQL query contract not met: %v", err)
	}
}

func actionReceiptInsertArgs(receipt ActionReceipt) []any {
	return []any{
		receipt.OrganizationID, receipt.IdempotencyKey, receipt.RequestSHA256, receipt.ConnectionID,
		receipt.ProviderKey, receipt.Operation, receipt.AttestationIssuer, receipt.AttestationKeyID,
		receipt.AuthorizationKind, receipt.AuthorizationID, receipt.ApprovalID, receipt.ActionID,
		receipt.ActorID, receipt.AttestationJTI, receipt.PayloadSHA256,
	}
}

func actionReceiptRow(receipt ActionReceipt, status, providerMessageID string, now time.Time) []any {
	return append(actionReceiptInsertArgs(receipt), status, providerMessageID, now, now)
}
