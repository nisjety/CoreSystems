package store

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgconn"
	"github.com/pashagolub/pgxmock/v4"

	"github.com/triodelab/integration-corev2/internal/zdr"
)

// These tests pin the SQL shape and the order of statements of the PostgreSQL implementation
// against pgxmock. The behaviour itself (constraints, the unique index, the guard trigger,
// locking under concurrency) is proved against a real database by the shared contract suite
// in internal/db/zdr_postgres_test.go, which needs INTEGRATION_TEST_DATABASE_URL.

var zdrRecordColumns = []string{"connection_id", "provider_key", "auth_mode", "capability", "basis", "basis_ref", "catalog_version", "digest", "checked_at", "created_at", "updated_at"}

var zdrAckColumns = []string{"id", "organization_id", "connection_id", "user_id", "provider_key", "capability", "basis", "catalog_version", "digest", "text_version", "locale", "text_sha256", "text_snapshot", "org_zdr_at_ack", "acknowledged_via", "request_id", "acknowledged_at", "revoked_at", "revoke_reason"}

var zdrLockColumns = append(append([]string{}, zdrRecordColumns...), "organization_id", "user_id", "deleted_at")

func testAssessment(t *testing.T, version int) ConnectionZDRAssessment {
	t.Helper()
	digest, err := zdr.Digest("openai-codex-subscription", "codex_chatgpt_device_code", zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, version)
	if err != nil {
		t.Fatal(err)
	}
	return ConnectionZDRAssessment{
		ConnectionID: "conn-1", ProviderKey: "openai-codex-subscription", AuthMode: "codex_chatgpt_device_code",
		Capability: zdr.CapabilityNotSupported, Basis: zdr.BasisProviderTermsKnown, BasisRef: "openai-codex-subscription@1",
		CatalogVersion: version, Digest: digest,
	}
}

func recordRow(a ConnectionZDRAssessment, at time.Time) []any {
	return []any{a.ConnectionID, a.ProviderKey, a.AuthMode, string(a.Capability), string(a.Basis), a.BasisRef, a.CatalogVersion, a.Digest, at, at, at}
}

func ackRow(id, digest string, revoked *time.Time, reason string, at time.Time) []any {
	return []any{id, "org-1", "conn-1", "user-1", "openai-codex-subscription", "not_supported", "provider_terms_known", 1, digest,
		"2026-10-a", "no", strings.Repeat("a", 64), "text", true, "connect", "req-1", at, revoked, reason}
}

func newZDRMock(t *testing.T) (pgxmock.PgxPoolIface, *PostgresRepository) {
	t.Helper()
	mock, err := pgxmock.NewPool()
	if err != nil {
		t.Fatalf("new pgx mock pool: %v", err)
	}
	t.Cleanup(mock.Close)
	return mock, &PostgresRepository{pool: mock}
}

const (
	sqlLockRecord   = `SELECT connection_id, .* FROM integration_connection_zdr WHERE connection_id = \$1 FOR UPDATE`
	sqlInsertRecord = `INSERT INTO integration_connection_zdr \(.*\) VALUES .* ON CONFLICT \(connection_id\) DO NOTHING RETURNING`
	sqlUpdateRecord = `UPDATE integration_connection_zdr SET provider_key`
	sqlSweepAcks    = `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = \$3, revoke_reason = \$4 WHERE connection_id = \$1 AND revoked_at IS NULL AND digest <> \$2 RETURNING`
)

func flat(sql string) string { return strings.Join(strings.Fields(sql), " ") }

func TestPostgresApplyCreatesTheRecordInsideItsOwnTransaction(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	at := time.Now().UTC()
	mock.MatchExpectationsInOrder(true)
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns))
	mock.ExpectQuery(sqlInsertRecord).WithArgs("conn-1", a.ProviderKey, a.AuthMode, "not_supported", "provider_terms_known", a.BasisRef, 1, a.Digest, pgxmock.AnyArg()).
		WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(a, at)...))
	mock.ExpectQuery(sqlSweepAcks).WithArgs("conn-1", a.Digest, pgxmock.AnyArg(), ZDRRevokeCapabilityChanged).WillReturnRows(pgxmock.NewRows(zdrAckColumns))
	mock.ExpectCommit()

	result, err := repo.ApplyConnectionZDRAssessment(t.Context(), a)
	if err != nil || !result.Created || result.Changed || result.Previous != nil || result.Record.Digest != a.Digest || len(result.Revoked) != 0 {
		t.Fatalf("apply = (%+v, %v)", result, err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresApplyOfAnUnchangedAssessmentWritesNoRecordButStillSweepsStaleAcknowledgements(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	at := time.Now().UTC().Add(-time.Hour)
	stale := time.Now().UTC().Add(-time.Minute)
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(a, at)...))
	// No INSERT and no UPDATE of the record is expected: pgxmock fails on any other statement.
	revokedAt := time.Now().UTC()
	mock.ExpectQuery(sqlSweepAcks).WithArgs("conn-1", a.Digest, pgxmock.AnyArg(), ZDRRevokeCapabilityChanged).
		WillReturnRows(pgxmock.NewRows(zdrAckColumns).AddRow(ackRow("ack-stale", strings.Repeat("b", 64), &revokedAt, ZDRRevokeCapabilityChanged, stale)...))
	mock.ExpectCommit()

	result, err := repo.ApplyConnectionZDRAssessment(t.Context(), a)
	if err != nil {
		t.Fatalf("apply: %v", err)
	}
	if result.Created || result.Changed || result.Previous == nil || len(result.Revoked) != 1 || result.Revoked[0].ID != "ack-stale" {
		t.Fatalf("apply = %+v, want an unchanged record and the stale acknowledgement swept", result)
	}
	if !result.Record.CheckedAt.Equal(at) {
		t.Fatalf("an unchanged assessment rewrote checked_at: %v, want %v", result.Record.CheckedAt, at)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresApplyOfAChangedDigestUpdatesTheRecordAndRevokesInTheSameTransaction(t *testing.T) {
	mock, repo := newZDRMock(t)
	old := testAssessment(t, 1)
	next := testAssessment(t, 2)
	created := time.Now().UTC().Add(-48 * time.Hour)
	revokedAt := time.Now().UTC()
	mock.MatchExpectationsInOrder(true)
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(old, created)...))
	mock.ExpectQuery(sqlUpdateRecord).WithArgs("conn-1", next.ProviderKey, next.AuthMode, "not_supported", "provider_terms_known", next.BasisRef, 2, next.Digest, pgxmock.AnyArg()).
		WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(next, time.Now().UTC())...))
	mock.ExpectQuery(sqlSweepAcks).WithArgs("conn-1", next.Digest, pgxmock.AnyArg(), ZDRRevokeCapabilityChanged).
		WillReturnRows(pgxmock.NewRows(zdrAckColumns).AddRow(ackRow("ack-1", old.Digest, &revokedAt, ZDRRevokeCapabilityChanged, created)...))
	mock.ExpectCommit()

	result, err := repo.ApplyConnectionZDRAssessment(t.Context(), next)
	if err != nil {
		t.Fatalf("apply: %v", err)
	}
	if result.Created || !result.Changed || result.Previous == nil || result.Previous.Digest != old.Digest || result.Record.Digest != next.Digest {
		t.Fatalf("apply = %+v, want a digest change from v1 to v2", result)
	}
	if len(result.Revoked) != 1 || result.Revoked[0].ID != "ack-1" || result.Revoked[0].RevokeReason != ZDRRevokeCapabilityChanged || result.Revoked[0].RevokedAt == nil {
		t.Fatalf("revoked = %+v", result.Revoked)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresApplyJoinsAnOpenAuditTransactionInsteadOfOpeningAnother(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	at := time.Now().UTC()
	mock.MatchExpectationsInOrder(true)
	mock.ExpectBegin() // the audit transaction: the ONLY Begin
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns))
	mock.ExpectQuery(sqlInsertRecord).WithArgs("conn-1", a.ProviderKey, a.AuthMode, "not_supported", "provider_terms_known", a.BasisRef, 1, a.Digest, pgxmock.AnyArg()).
		WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(a, at)...))
	mock.ExpectQuery(sqlSweepAcks).WithArgs("conn-1", a.Digest, pgxmock.AnyArg(), ZDRRevokeCapabilityChanged).WillReturnRows(pgxmock.NewRows(zdrAckColumns))
	mock.ExpectExec(`INSERT INTO integration_audit_events`).WithArgs("audit-1", "org-1", "", "conn-1", "zdr", "", pgxmock.AnyArg(), "", pgxmock.AnyArg()).
		WillReturnResult(pgxmock.NewResult("INSERT", 1))
	mock.ExpectCommit()

	err := repo.WithAuditTransaction(t.Context(), func(tx AuditTransaction) error {
		if _, applyErr := tx.ApplyConnectionZDRAssessment(t.Context(), a); applyErr != nil {
			return applyErr
		}
		return tx.InsertAuditEvent(t.Context(), AuditEvent{ID: "audit-1", OrganizationID: "org-1", ConnectionID: "conn-1", EventType: "zdr", CreatedAt: time.Now().UTC()})
	})
	if err != nil {
		t.Fatalf("WithAuditTransaction: %v", err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations (a second Begin, or a missing statement): %v", err)
	}
}

func TestPostgresApplyRollsBackWhenTheSweepFails(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	wantErr := errors.New("revocation unavailable")
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(a, time.Now().UTC())...))
	mock.ExpectQuery(sqlSweepAcks).WithArgs("conn-1", a.Digest, pgxmock.AnyArg(), ZDRRevokeCapabilityChanged).WillReturnError(wantErr)
	mock.ExpectRollback()
	if _, err := repo.ApplyConnectionZDRAssessment(t.Context(), a); !errors.Is(err, wantErr) {
		t.Fatalf("apply error = %v, want %v", err, wantErr)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresApplyMapsAMissingConnectionToNotFound(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns))
	mock.ExpectQuery(sqlInsertRecord).WithArgs("conn-1", a.ProviderKey, a.AuthMode, "not_supported", "provider_terms_known", a.BasisRef, 1, a.Digest, pgxmock.AnyArg()).
		WillReturnError(&pgconn.PgError{Code: "23503", Message: "insert violates foreign key constraint"})
	mock.ExpectRollback()
	if _, err := repo.ApplyConnectionZDRAssessment(t.Context(), a); !errors.Is(err, ErrNotFound) {
		t.Fatalf("apply error = %v, want ErrNotFound", err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresApplyFollowsAConcurrentFirstInsert(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	at := time.Now().UTC().Add(-time.Second)
	mock.MatchExpectationsInOrder(true)
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns))
	// DO NOTHING returned no row: another transaction inserted first.
	mock.ExpectQuery(sqlInsertRecord).WithArgs("conn-1", a.ProviderKey, a.AuthMode, "not_supported", "provider_terms_known", a.BasisRef, 1, a.Digest, pgxmock.AnyArg()).
		WillReturnRows(pgxmock.NewRows(zdrRecordColumns))
	mock.ExpectQuery(sqlLockRecord).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(a, at)...))
	mock.ExpectQuery(sqlSweepAcks).WithArgs("conn-1", a.Digest, pgxmock.AnyArg(), ZDRRevokeCapabilityChanged).WillReturnRows(pgxmock.NewRows(zdrAckColumns))
	mock.ExpectCommit()
	result, err := repo.ApplyConnectionZDRAssessment(t.Context(), a)
	if err != nil || result.Created || result.Changed || result.Previous == nil {
		t.Fatalf("apply = (%+v, %v), want the other writer's record treated as existing and unchanged", result, err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresApplyRefusesAMalformedAssessmentBeforeAnyStatement(t *testing.T) {
	_, repo := newZDRMock(t) // no expectations: any statement fails the test
	a := testAssessment(t, 1)
	a.Digest = strings.Repeat("0", 64)
	if _, err := repo.ApplyConnectionZDRAssessment(t.Context(), a); !errors.Is(err, ErrInvalidZDR) {
		t.Fatalf("apply error = %v, want ErrInvalidZDR", err)
	}
}

func TestPostgresGetConnectionZDRMapsNoRowsToNotFound(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	at := time.Now().UTC()
	mock.ExpectQuery(`FROM integration_connection_zdr WHERE connection_id = \$1`).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrRecordColumns).AddRow(recordRow(a, at)...))
	mock.ExpectQuery(`FROM integration_connection_zdr WHERE connection_id = \$1`).WithArgs("conn-2").WillReturnRows(pgxmock.NewRows(zdrRecordColumns))
	got, err := repo.GetConnectionZDR(t.Context(), "conn-1")
	if err != nil || got.Digest != a.Digest || got.Capability != zdr.CapabilityNotSupported || got.Basis != zdr.BasisProviderTermsKnown {
		t.Fatalf("GetConnectionZDR = (%+v, %v)", got, err)
	}
	if _, err := repo.GetConnectionZDR(t.Context(), "conn-2"); !errors.Is(err, ErrNotFound) {
		t.Fatalf("GetConnectionZDR of an unassessed connection = %v, want ErrNotFound", err)
	}
}

func testAck(t *testing.T, digest string) NewConnectionZDRAcknowledgement {
	t.Helper()
	text := "text"
	return NewConnectionZDRAcknowledgement{
		OrganizationID: "org-1", ConnectionID: "conn-1", UserID: "user-1", Digest: digest, TextVersion: "2026-10-a", Locale: "no",
		TextSHA256:   "3f0ae59f0dbbf6e32eb3e2d8d2d7aa7f3c9a9c1f0d6e0e6b1a0b2c3d4e5f6a7b",
		TextSnapshot: text, OrgZDRAtAck: true, AcknowledgedVia: ZDRAcknowledgedViaConnect, RequestID: "req-1", ID: "ack-1",
	}
}

func rehash(t *testing.T, in NewConnectionZDRAcknowledgement) NewConnectionZDRAcknowledgement {
	t.Helper()
	in.TextSHA256 = sha256Of(in.TextSnapshot)
	return in
}

func lockRow(a ConnectionZDRAssessment, org, user string, deletedAt *time.Time) []any {
	return append(recordRow(a, time.Now().UTC()), org, user, deletedAt)
}

const (
	sqlLockForAck = `SELECT z\.connection_id, .* FROM integration_connection_zdr AS z JOIN integration_connections AS c ON c\.id = z\.connection_id WHERE z\.connection_id = \$1 FOR SHARE OF z`
	sqlInsertAck  = `INSERT INTO integration_connection_zdr_acknowledgements \(.*\) VALUES .* ON CONFLICT \(connection_id, digest\) WHERE revoked_at IS NULL DO NOTHING RETURNING`
)

func TestPostgresAcknowledgementLocksTheRecordSharedThenInserts(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	in := rehash(t, testAck(t, a.Digest))
	at := time.Now().UTC()
	mock.MatchExpectationsInOrder(true)
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockForAck).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrLockColumns).AddRow(lockRow(a, "org-1", "user-1", nil)...))
	mock.ExpectQuery(sqlInsertAck).WithArgs("ack-1", "org-1", "conn-1", "user-1", a.ProviderKey, "not_supported", "provider_terms_known", 1, a.Digest,
		"2026-10-a", "no", in.TextSHA256, "text", true, "connect", "req-1", pgxmock.AnyArg()).
		WillReturnRows(pgxmock.NewRows(zdrAckColumns).AddRow(ackRow("ack-1", a.Digest, nil, "", at)...))
	mock.ExpectCommit()
	saved, created, err := repo.InsertConnectionZDRAcknowledgement(t.Context(), in)
	if err != nil || !created || saved.ID != "ack-1" || saved.RevokedAt != nil || saved.Digest != a.Digest {
		t.Fatalf("insert = (%+v, created=%v, %v)", saved, created, err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresAcknowledgementReplayReturnsTheStoredRow(t *testing.T) {
	mock, repo := newZDRMock(t)
	a := testAssessment(t, 1)
	in := rehash(t, testAck(t, a.Digest))
	at := time.Now().UTC()
	mock.MatchExpectationsInOrder(true)
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockForAck).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrLockColumns).AddRow(lockRow(a, "org-1", "user-1", nil)...))
	mock.ExpectQuery(sqlInsertAck).WithArgs("ack-1", "org-1", "conn-1", "user-1", a.ProviderKey, "not_supported", "provider_terms_known", 1, a.Digest,
		"2026-10-a", "no", in.TextSHA256, "text", true, "connect", "req-1", pgxmock.AnyArg()).
		WillReturnRows(pgxmock.NewRows(zdrAckColumns)) // DO NOTHING: an active row exists
	mock.ExpectQuery(`FROM integration_connection_zdr_acknowledgements WHERE connection_id = \$1 AND digest = \$2 AND revoked_at IS NULL`).WithArgs("conn-1", a.Digest).
		WillReturnRows(pgxmock.NewRows(zdrAckColumns).AddRow(ackRow("ack-original", a.Digest, nil, "", at)...))
	mock.ExpectCommit()
	saved, created, err := repo.InsertConnectionZDRAcknowledgement(t.Context(), in)
	if err != nil || created || saved.ID != "ack-original" {
		t.Fatalf("replay = (%+v, created=%v, %v), want the stored row and created=false", saved, created, err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

func TestPostgresAcknowledgementRefusesStaleOwnerlessAndDeletedBeforeInserting(t *testing.T) {
	a := testAssessment(t, 1)
	deleted := time.Now().UTC()
	cases := []struct {
		name string
		rows func() *pgxmock.Rows
		in   func() NewConnectionZDRAcknowledgement
		want error
	}{
		{"no record", func() *pgxmock.Rows { return pgxmock.NewRows(zdrLockColumns) }, func() NewConnectionZDRAcknowledgement { return rehash(t, testAck(t, a.Digest)) }, ErrNotFound},
		{"another user", func() *pgxmock.Rows {
			return pgxmock.NewRows(zdrLockColumns).AddRow(lockRow(a, "org-1", "owner", nil)...)
		}, func() NewConnectionZDRAcknowledgement { return rehash(t, testAck(t, a.Digest)) }, ErrNotFound},
		{"another organisation", func() *pgxmock.Rows {
			return pgxmock.NewRows(zdrLockColumns).AddRow(lockRow(a, "org-9", "user-1", nil)...)
		}, func() NewConnectionZDRAcknowledgement { return rehash(t, testAck(t, a.Digest)) }, ErrNotFound},
		{"deleted connection", func() *pgxmock.Rows {
			return pgxmock.NewRows(zdrLockColumns).AddRow(lockRow(a, "org-1", "user-1", &deleted)...)
		}, func() NewConnectionZDRAcknowledgement { return rehash(t, testAck(t, a.Digest)) }, ErrNotFound},
		{"stale digest", func() *pgxmock.Rows {
			return pgxmock.NewRows(zdrLockColumns).AddRow(lockRow(a, "org-1", "user-1", nil)...)
		}, func() NewConnectionZDRAcknowledgement { return rehash(t, testAck(t, strings.Repeat("c", 64))) }, ErrConflict},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			mock, repo := newZDRMock(t)
			mock.ExpectBegin()
			mock.ExpectQuery(sqlLockForAck).WithArgs("conn-1").WillReturnRows(tc.rows())
			mock.ExpectRollback() // nothing may be inserted: pgxmock fails on an INSERT
			if _, _, err := repo.InsertConnectionZDRAcknowledgement(t.Context(), tc.in()); !errors.Is(err, tc.want) {
				t.Fatalf("err = %v, want %v", err, tc.want)
			}
			if err := mock.ExpectationsWereMet(); err != nil {
				t.Fatalf("unmet expectations: %v", err)
			}
		})
	}
}

func TestPostgresAcknowledgementOfASupportedConnectionIsInvalid(t *testing.T) {
	mock, repo := newZDRMock(t)
	digest, _ := zdr.Digest("p", "a", zdr.CapabilitySupported, zdr.BasisVerevonAttestedDeployment, 1)
	supported := ConnectionZDRAssessment{ConnectionID: "conn-1", ProviderKey: "p", AuthMode: "a", Capability: zdr.CapabilitySupported, Basis: zdr.BasisVerevonAttestedDeployment, BasisRef: "p@1", CatalogVersion: 1, Digest: digest}
	mock.ExpectBegin()
	mock.ExpectQuery(sqlLockForAck).WithArgs("conn-1").WillReturnRows(pgxmock.NewRows(zdrLockColumns).AddRow(lockRow(supported, "org-1", "user-1", nil)...))
	mock.ExpectRollback()
	if _, _, err := repo.InsertConnectionZDRAcknowledgement(t.Context(), rehash(t, testAck(t, digest))); !errors.Is(err, ErrInvalidZDR) {
		t.Fatalf("err = %v, want ErrInvalidZDR", err)
	}
}

func TestPostgresAcknowledgementValidationHappensBeforeAnyStatement(t *testing.T) {
	_, repo := newZDRMock(t)
	bad := testAck(t, strings.Repeat("a", 64)) // its text hash is not the hash of its text
	if _, _, err := repo.InsertConnectionZDRAcknowledgement(t.Context(), bad); !errors.Is(err, ErrInvalidZDR) {
		t.Fatalf("err = %v, want ErrInvalidZDR", err)
	}
}

func TestPostgresListAndRevokeAcknowledgements(t *testing.T) {
	mock, repo := newZDRMock(t)
	now := time.Now().UTC()
	older := now.Add(-time.Hour)
	revoked := now
	mock.ExpectQuery(`FROM integration_connection_zdr_acknowledgements WHERE connection_id = \$1 AND \(\$2::boolean = FALSE OR revoked_at IS NULL\) ORDER BY acknowledged_at DESC, id ASC`).WithArgs("conn-1", false).
		WillReturnRows(pgxmock.NewRows(zdrAckColumns).
			AddRow(ackRow("ack-old", strings.Repeat("a", 64), &revoked, ZDRRevokeOwnerRevoked, older)...).
			AddRow(ackRow("ack-new", strings.Repeat("b", 64), nil, "", now)...))
	rows, err := repo.ListConnectionZDRAcknowledgements(t.Context(), "conn-1", false)
	if err != nil || len(rows) != 2 || rows[0].ID != "ack-new" || rows[1].ID != "ack-old" || rows[1].RevokedAt == nil || rows[0].Active() == false {
		t.Fatalf("list = (%+v, %v), want newest first with revocation decoded", rows, err)
	}

	mock.ExpectQuery(`UPDATE integration_connection_zdr_acknowledgements SET revoked_at = \$2, revoke_reason = \$3 WHERE connection_id = \$1 AND revoked_at IS NULL RETURNING`).
		WithArgs("conn-1", pgxmock.AnyArg(), ZDRRevokeConnectionRemoved).
		WillReturnRows(pgxmock.NewRows(zdrAckColumns).AddRow(ackRow("ack-new", strings.Repeat("b", 64), &revoked, ZDRRevokeConnectionRemoved, now)...))
	got, err := repo.RevokeConnectionZDRAcknowledgements(t.Context(), "conn-1", ZDRRevokeConnectionRemoved, time.Time{})
	if err != nil || len(got) != 1 || got[0].RevokeReason != ZDRRevokeConnectionRemoved {
		t.Fatalf("revoke = (%+v, %v)", got, err)
	}
	if _, err := repo.RevokeConnectionZDRAcknowledgements(t.Context(), "conn-1", "because", time.Time{}); !errors.Is(err, ErrInvalidZDR) {
		t.Fatalf("revoke with an invented reason = %v, want ErrInvalidZDR before any statement", err)
	}
	if err := mock.ExpectationsWereMet(); err != nil {
		t.Fatalf("unmet expectations: %v", err)
	}
}

// The generic connection write paths are the very read-modify-write the side table exists to
// keep clear of. A source scan keeps it that way: nobody can add a ZDR column to them, or to
// connectionColumns, without this failing and asking why.
func TestGenericConnectionSQLNeverNamesTheZDRTables(t *testing.T) {
	// (memory.go holds the in-memory ZDR maps; its generic writes are held to the same rule by
	// the behavioural contract suite, which runs against both implementations.)
	for _, file := range []string{"postgres.go", "postgres_platform.go"} {
		raw, err := os.ReadFile(file)
		if err != nil {
			t.Fatalf("read %s: %v", file, err)
		}
		if strings.Contains(strings.ToLower(string(raw)), "zdr") {
			t.Errorf("%s mentions zdr: the generic connection paths must stay clear of the capability record (see zdr.go)", file)
		}
	}
	if strings.Contains(strings.ToLower(connectionColumns), "zdr") {
		t.Error("connectionColumns names a zdr column")
	}
}

func TestConnectionHasNoZDRFieldAndSerialisesNone(t *testing.T) {
	typ := reflect.TypeOf(Connection{})
	for index := 0; index < typ.NumField(); index++ {
		field := typ.Field(index)
		for _, name := range []string{field.Name, field.Tag.Get("json")} {
			if strings.Contains(strings.ToLower(name), "zdr") {
				t.Errorf("store.Connection has a field or JSON name %q about ZDR: capability belongs to the side table, not the connection row", name)
			}
		}
	}
	encoded, err := json.Marshal(Connection{ID: "c", ProviderContext: map[string]string{"k": "v"}})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(strings.ToLower(string(encoded)), "zdr") {
		t.Errorf("a serialised connection carries a ZDR field: %s", encoded)
	}
}

func sha256Of(text string) string {
	sum := sha256.Sum256([]byte(text))
	return hex.EncodeToString(sum[:])
}
