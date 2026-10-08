package db_test

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/integration-corev2/internal/db"
	"github.com/triodelab/integration-corev2/internal/store"
	"github.com/triodelab/integration-corev2/internal/store/storetest"
	"github.com/triodelab/integration-corev2/internal/zdr"
)

// These tests run migration 0013 and the PostgreSQL ZDR store against a real database. They need
// INTEGRATION_TEST_DATABASE_URL (a disposable database whose name contains test or tmp) and
// FAIL, not skip, under REQUIRE_CONTRACT or CI. Each test works in its own schema.

const (
	digestA = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	digestB = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
)

func sqlState(err error) string {
	var pgErr *pgconn.PgError
	if errors.As(err, &pgErr) {
		return pgErr.Code
	}
	return ""
}

func seedConnection(t *testing.T, pool *pgxpool.Pool, id string) {
	t.Helper()
	_, err := pool.Exec(context.Background(), `
		INSERT INTO integration_connections (id, provider_key, connector_type, organization_id, workspace_id, user_id, status, encrypted_access_token, access_token_expires_at)
		VALUES ($1, 'openai-codex-subscription', 'openai-codex-subscription', 'org-1', 'ws-1', 'user-1', 'active', '', now() + interval '1 hour')`, id)
	if err != nil {
		t.Fatalf("seed connection %s: %v", id, err)
	}
}

func insertRecord(pool *pgxpool.Pool, connection, capability, digest string) error {
	_, err := pool.Exec(context.Background(), `
		INSERT INTO integration_connection_zdr (connection_id, provider_key, auth_mode, capability, basis, basis_ref, catalog_version, digest, checked_at)
		VALUES ($1, 'openai-codex-subscription', 'codex_chatgpt_device_code', $2, 'provider_terms_known', 'openai-codex-subscription@1', 1, $3, now())`,
		connection, capability, digest)
	return err
}

func insertAck(pool *pgxpool.Pool, id, connection, capability, digest string) error {
	_, err := pool.Exec(context.Background(), `
		INSERT INTO integration_connection_zdr_acknowledgements
			(id, organization_id, connection_id, user_id, provider_key, capability, basis, catalog_version, digest,
			 text_version, locale, text_sha256, text_snapshot, org_zdr_at_ack, acknowledged_via)
		VALUES ($1, 'org-1', $2, 'user-1', 'openai-codex-subscription', $3, 'provider_terms_known', 1, $4,
			 'v1', 'no', $5, 'text', true, 'connect')`, id, connection, capability, digest, strings.Repeat("c", 64))
	return err
}

func TestZDRMigrationConstraintsOnDisposablePostgres(t *testing.T) {
	ctx := context.Background()
	pool := storetest.IsolatedPostgres(t, true)

	// Applying again, and a third time, is a no-op.
	for i := 0; i < 2; i++ {
		if err := db.ApplyMigrations(ctx, pool); err != nil {
			t.Fatalf("re-apply %d: %v", i+1, err)
		}
	}
	var recorded bool
	if err := pool.QueryRow(ctx, `SELECT EXISTS (SELECT 1 FROM integration_schema_migrations WHERE version = '0013_model_connection_zdr.sql')`).Scan(&recorded); err != nil || !recorded {
		t.Fatalf("0013 not recorded (%v, %v)", recorded, err)
	}

	conn := "conn-" + uuid.NewString()
	seedConnection(t, pool, conn)

	t.Run("the side table refuses what is not a capability, a digest or a connection", func(t *testing.T) {
		if err := insertRecord(pool, conn, "mostly", digestA); sqlState(err) != "23514" {
			t.Errorf("unknown capability: %v, want a check violation", err)
		}
		if err := insertRecord(pool, conn, "not_supported", "abc"); sqlState(err) != "23514" {
			t.Errorf("malformed digest: %v, want a check violation", err)
		}
		if err := insertRecord(pool, conn, "not_supported", strings.ToUpper(digestA)); sqlState(err) != "23514" {
			t.Errorf("upper-case digest: %v, want a check violation", err)
		}
		if err := insertRecord(pool, "no-such-connection", "not_supported", digestA); sqlState(err) != "23503" {
			t.Errorf("unknown connection: %v, want a foreign key violation", err)
		}
		if err := insertRecord(pool, conn, "not_supported", digestA); err != nil {
			t.Fatalf("a valid record: %v", err)
		}
		if err := insertRecord(pool, conn, "not_supported", digestA); sqlState(err) != "23505" {
			t.Errorf("a second record for the connection: %v, want a unique violation", err)
		}
	})

	t.Run("the ledger has no foreign key and takes only acknowledgeable capabilities", func(t *testing.T) {
		var foreignKeys int
		if err := pool.QueryRow(ctx, `SELECT count(*) FROM pg_constraint WHERE conrelid = 'integration_connection_zdr_acknowledgements'::regclass AND contype = 'f'`).Scan(&foreignKeys); err != nil || foreignKeys != 0 {
			t.Fatalf("the ledger has %d foreign keys (%v), want none", foreignKeys, err)
		}
		var onDelete string
		if err := pool.QueryRow(ctx, `SELECT confdeltype::text FROM pg_constraint WHERE conrelid = 'integration_connection_zdr'::regclass AND contype = 'f'`).Scan(&onDelete); err != nil || onDelete != "c" {
			t.Fatalf("the side table foreign key on delete = %q (%v), want cascade (c)", onDelete, err)
		}
		if err := insertAck(pool, "ack-supported", conn, "supported", digestA); sqlState(err) != "23514" {
			t.Errorf("an acknowledgement of a supported connection: %v, want a check violation", err)
		}
		if err := insertAck(pool, "ack-bad-digest", conn, "not_supported", "xyz"); sqlState(err) != "23514" {
			t.Errorf("an acknowledgement with a malformed digest: %v, want a check violation", err)
		}
		// Not a foreign key: a row may name a connection that does not exist (a hard-deleted one).
		if err := insertAck(pool, "ack-orphan", "connection-that-never-existed", "not_supported", digestA); err != nil {
			t.Errorf("an acknowledgement of an unknown connection: %v, want it accepted (the ledger depends on no connection row)", err)
		}
	})

	t.Run("one active acknowledgement per connection and digest, and revocation frees the slot", func(t *testing.T) {
		if err := insertAck(pool, "ack-1", conn, "not_supported", digestA); err != nil {
			t.Fatalf("first acknowledgement: %v", err)
		}
		if err := insertAck(pool, "ack-2", conn, "not_supported", digestA); sqlState(err) != "23505" {
			t.Errorf("a second active acknowledgement of the same digest: %v, want a unique violation", err)
		}
		if err := insertAck(pool, "ack-3", conn, "not_supported", digestB); err != nil {
			t.Errorf("an active acknowledgement of another digest: %v", err)
		}
		if _, err := pool.Exec(ctx, `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = now(), revoke_reason = 'owner_revoked' WHERE id = 'ack-1'`); err != nil {
			t.Fatalf("revoke: %v", err)
		}
		if err := insertAck(pool, "ack-4", conn, "not_supported", digestA); err != nil {
			t.Errorf("a new acknowledgement after the revocation: %v", err)
		}
	})

	t.Run("a revocation needs a closed reason and a timestamp together", func(t *testing.T) {
		if err := insertAck(pool, "ack-r1", conn, "not_supported", strings.Repeat("d", 64)); err != nil {
			t.Fatal(err)
		}
		if _, err := pool.Exec(ctx, `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = now() WHERE id = 'ack-r1'`); sqlState(err) != "23514" {
			t.Errorf("revoked_at without a reason: %v, want a check violation", err)
		}
		if _, err := pool.Exec(ctx, `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = now(), revoke_reason = 'because' WHERE id = 'ack-r1'`); sqlState(err) != "23514" {
			t.Errorf("an invented reason: %v, want a check violation", err)
		}
		if _, err := pool.Exec(ctx, `UPDATE integration_connection_zdr_acknowledgements SET revoke_reason = 'owner_revoked' WHERE id = 'ack-r1'`); sqlState(err) != "23514" {
			t.Errorf("a reason without a timestamp: %v, want a check violation", err)
		}
	})

	t.Run("the guard makes the ledger append-only apart from one revocation", func(t *testing.T) {
		if err := insertAck(pool, "ack-g1", conn, "not_supported", strings.Repeat("e", 64)); err != nil {
			t.Fatal(err)
		}
		for name, statement := range map[string]string{
			"rewriting the recorded text":    `UPDATE integration_connection_zdr_acknowledgements SET text_snapshot = 'edited' WHERE id = 'ack-g1'`,
			"rewriting the digest":           `UPDATE integration_connection_zdr_acknowledgements SET digest = '` + digestB + `' WHERE id = 'ack-g1'`,
			"moving it to another user":      `UPDATE integration_connection_zdr_acknowledgements SET user_id = 'someone-else' WHERE id = 'ack-g1'`,
			"changing when it was accepted":  `UPDATE integration_connection_zdr_acknowledgements SET acknowledged_at = now() - interval '1 year' WHERE id = 'ack-g1'`,
			"revoking and rewriting at once": `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = now(), revoke_reason = 'owner_revoked', text_snapshot = 'edited' WHERE id = 'ack-g1'`,
		} {
			if _, err := pool.Exec(ctx, statement); sqlState(err) != "23514" || !strings.Contains(errorText(err), "only the revocation columns") {
				t.Errorf("%s: %v, want the append-only guard", name, err)
			}
		}
		if _, err := pool.Exec(ctx, `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = now(), revoke_reason = 'capability_changed' WHERE id = 'ack-g1'`); err != nil {
			t.Fatalf("the one allowed update, a revocation: %v", err)
		}
		for name, statement := range map[string]string{
			"revoking again":         `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = now(), revoke_reason = 'owner_revoked' WHERE id = 'ack-g1'`,
			"un-revoking":            `UPDATE integration_connection_zdr_acknowledgements SET revoked_at = NULL, revoke_reason = '' WHERE id = 'ack-g1'`,
			"touching a revoked row": `UPDATE integration_connection_zdr_acknowledgements SET request_id = 'x' WHERE id = 'ack-g1'`,
		} {
			if _, err := pool.Exec(ctx, statement); sqlState(err) != "23514" || !strings.Contains(errorText(err), "final") {
				t.Errorf("%s: %v, want a revoked row to be final", name, err)
			}
		}
	})

	t.Run("deleting a connection row drops its record and leaves the ledger", func(t *testing.T) {
		hard := "conn-" + uuid.NewString()
		seedConnection(t, pool, hard)
		if err := insertRecord(pool, hard, "not_supported", digestA); err != nil {
			t.Fatal(err)
		}
		if err := insertAck(pool, "ack-hard-"+uuid.NewString(), hard, "not_supported", digestA); err != nil {
			t.Fatal(err)
		}
		if _, err := pool.Exec(ctx, `DELETE FROM integration_connections WHERE id = $1`, hard); err != nil {
			t.Fatalf("hard delete: %v", err)
		}
		var records, acks int
		if err := pool.QueryRow(ctx, `SELECT (SELECT count(*) FROM integration_connection_zdr WHERE connection_id = $1), (SELECT count(*) FROM integration_connection_zdr_acknowledgements WHERE connection_id = $1)`, hard).Scan(&records, &acks); err != nil {
			t.Fatal(err)
		}
		if records != 0 || acks != 1 {
			t.Fatalf("after a hard delete: %d records and %d ledger rows, want 0 and 1", records, acks)
		}
	})
}

func errorText(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}

func assessmentFor(connection string) (store.ConnectionZDRAssessment, error) {
	digest, err := zdr.Digest("openai-codex-subscription", "codex_chatgpt_device_code", zdr.CapabilityNotSupported, zdr.BasisProviderTermsKnown, 1)
	if err != nil {
		return store.ConnectionZDRAssessment{}, err
	}
	return store.ConnectionZDRAssessment{
		ConnectionID: connection, ProviderKey: "openai-codex-subscription", AuthMode: "codex_chatgpt_device_code",
		Capability: zdr.CapabilityNotSupported, Basis: zdr.BasisProviderTermsKnown, BasisRef: "openai-codex-subscription@1",
		CatalogVersion: 1, Digest: digest,
	}, nil
}

// A database at 0012, with the shape of the live data, upgrades by applying 0013 alone: the
// tables appear empty, no existing row is touched, and a second run changes nothing.
func TestZDRMigrationUpgradesA0012DatabaseWithoutTouchingItsData(t *testing.T) {
	ctx := context.Background()
	pool := storetest.IsolatedPostgres(t, true)
	repo := store.NewPostgresRepository(pool)

	// Put the schema back where a 0012 database is: no 0013 objects, no 0013 record.
	for _, statement := range []string{
		`DROP TABLE integration_connection_zdr_acknowledgements`,
		`DROP TABLE integration_connection_zdr`,
		`DROP FUNCTION integration_connection_zdr_ack_guard()`,
		`DELETE FROM integration_schema_migrations WHERE version = '0013_model_connection_zdr.sql'`,
	} {
		if _, err := pool.Exec(ctx, statement); err != nil {
			t.Fatalf("rewind to 0012 (%s): %v", statement, err)
		}
	}
	var present bool
	if err := pool.QueryRow(ctx, `SELECT to_regclass('integration_connection_zdr') IS NOT NULL`).Scan(&present); err != nil || present {
		t.Fatalf("the rewind left the 0013 table behind (%v, %v)", present, err)
	}

	// The live shape: 2 active, 2 pending, 1 needs_refresh and 2 deleted subscription rows.
	statuses := []string{"active", "active", "pending", "pending", "needs_refresh", "deleted", "deleted"}
	for i, status := range statuses {
		connection := store.Connection{
			ID: "legacy-" + uuid.NewString(), ProviderKey: "openai-codex-subscription", ConnectorType: "openai-codex-subscription",
			OrganizationID: "org-legacy", UserID: "user-" + string(rune('a'+i)), Status: status, Capabilities: []string{"model.inference"},
			ProviderContext: map[string]string{"auth_mode": "chatgpt_device_code"}, AccessTokenExpiresAt: time.Now().UTC().Add(time.Hour),
		}
		saved, err := repo.UpsertConnection(ctx, connection)
		if err != nil {
			t.Fatalf("seed %d: %v", i, err)
		}
		if status == "deleted" {
			if _, err := repo.MarkConnectionDeleted(ctx, saved.ID); err != nil {
				t.Fatalf("delete %d: %v", i, err)
			}
		}
	}
	snapshot := func() string {
		rows, err := pool.Query(ctx, `SELECT id || '|' || status || '|' || updated_at::text || '|' || coalesce(deleted_at::text, '') FROM integration_connections ORDER BY id`)
		if err != nil {
			t.Fatalf("snapshot: %v", err)
		}
		defer rows.Close()
		var lines []string
		for rows.Next() {
			var line string
			if err := rows.Scan(&line); err != nil {
				t.Fatal(err)
			}
			lines = append(lines, line)
		}
		return strings.Join(lines, "\n")
	}
	before := snapshot()
	if strings.Count(before, "\n") != len(statuses)-1 {
		t.Fatalf("seeded %d rows, snapshot = %s", len(statuses), before)
	}

	if err := db.ApplyMigrations(ctx, pool); err != nil {
		t.Fatalf("upgrade: %v", err)
	}
	var records, acks int
	if err := pool.QueryRow(ctx, `SELECT (SELECT count(*) FROM integration_connection_zdr), (SELECT count(*) FROM integration_connection_zdr_acknowledgements)`).Scan(&records, &acks); err != nil {
		t.Fatalf("count: %v", err)
	}
	if records != 0 || acks != 0 {
		t.Fatalf("the migration wrote %d records and %d acknowledgements; backfilling is the service's reconcile, not SQL", records, acks)
	}
	if after := snapshot(); after != before {
		t.Fatalf("the migration changed existing connection rows:\nbefore:\n%s\nafter:\n%s", before, after)
	}
	if err := db.ApplyMigrations(ctx, pool); err != nil {
		t.Fatalf("second start: %v", err)
	}
}

// The PostgreSQL repository passes the same ZDR contract suite as the in-memory one, including
// the concurrent cases that only a database can prove (a first-insert race, and an
// acknowledgement racing a digest change).
func TestPostgresConnectionZDRContractOnDisposablePostgres(t *testing.T) {
	pool := storetest.IsolatedPostgres(t, true)
	storetest.RunConnectionZDRContract(t, storetest.ZDRHarness{
		NewRepository: func(t *testing.T) store.Repository { return store.NewPostgresRepository(pool) },
		AuditEventExists: func(t *testing.T, _ store.Repository, id string) bool {
			t.Helper()
			var exists bool
			if err := pool.QueryRow(context.Background(), `SELECT EXISTS (SELECT 1 FROM integration_audit_events WHERE id = $1)`, id).Scan(&exists); err != nil {
				t.Fatalf("audit event lookup: %v", err)
			}
			return exists
		},
	})
}

// A stale active acknowledgement (one a missed revocation left behind) is swept by the next
// assessment even when the digest has not moved, so the ledger heals.
func TestPostgresApplyHealsAStaleActiveAcknowledgement(t *testing.T) {
	ctx := context.Background()
	pool := storetest.IsolatedPostgres(t, true)
	repo := store.NewPostgresRepository(pool)
	conn := "conn-" + uuid.NewString()
	seedConnection(t, pool, conn)
	if err := insertRecord(pool, conn, "not_supported", digestA); err != nil {
		t.Fatal(err)
	}
	if err := insertAck(pool, "ack-stale", conn, "not_supported", digestB); err != nil { // bound to another digest
		t.Fatal(err)
	}
	// The record's digest is not the digest of its fields here, so go through the repository
	// with a genuine assessment whose digest equals what is stored: set the record to it first.
	assessment, err := assessmentFor(conn)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := pool.Exec(ctx, `UPDATE integration_connection_zdr SET digest = $2 WHERE connection_id = $1`, conn, assessment.Digest); err != nil {
		t.Fatal(err)
	}
	result, err := repo.ApplyConnectionZDRAssessment(ctx, assessment)
	if err != nil {
		t.Fatalf("apply: %v", err)
	}
	if result.Changed || len(result.Revoked) != 1 || result.Revoked[0].ID != "ack-stale" || result.Revoked[0].RevokeReason != store.ZDRRevokeCapabilityChanged {
		t.Fatalf("apply = %+v, want the stale acknowledgement swept without a digest change", result)
	}
}
