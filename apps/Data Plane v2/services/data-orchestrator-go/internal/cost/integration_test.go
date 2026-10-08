package cost

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
)

// These tests run the statements of this package against a REAL Postgres: the consumer's INSERT
// and the forwarder's claim, mark and backlog statements, over the schema the migrations build
// (the two that created cost_events and the one that is the subject of this task, applied twice
// to prove it is idempotent). No part of what they check is stood in for. They need
// TEST_DATABASE_URL, a disposable local Postgres 16 (see tests/contract/postgres/doc.go): each
// test works in a schema of its own that is dropped afterwards.
//
// Without TEST_DATABASE_URL they skip, and FAIL when REQUIRE_CONTRACT is set: a database
// contract that cannot run must never read as one that passed. tests/scripts/
// run-postgres-contract-tests.sh does not run this module yet (it is listed there as "not wired
// yet"); until it is, run them by hand before deploying a change to the statements.

const costEventsMigrationVersion = "20261003140000_cost_events_v2.sql"

func requireTestDatabase(t *testing.T) string {
	t.Helper()
	raw := strings.TrimSpace(os.Getenv("TEST_DATABASE_URL"))
	if raw == "" {
		if os.Getenv("REQUIRE_CONTRACT") != "" {
			t.Fatal("TEST_DATABASE_URL is not set while REQUIRE_CONTRACT is: a database contract that cannot run must fail, never skip")
		}
		t.Skip("TEST_DATABASE_URL is required for disposable PostgreSQL integration tests")
	}
	parsed, err := url.Parse(raw)
	if err != nil || (parsed.Scheme != "postgres" && parsed.Scheme != "postgresql") {
		t.Fatal("TEST_DATABASE_URL must be a postgres:// URL")
	}
	if host := parsed.Hostname(); host != "localhost" && host != "127.0.0.1" && host != "::1" && !strings.HasPrefix(parsed.Query().Get("host"), "/") {
		t.Fatalf("refusing a non-local test database host %q", host)
	}
	return raw
}

func readMigration(t *testing.T, name string) string {
	t.Helper()
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("locate the test source")
	}
	path := filepath.Join(filepath.Dir(source), "..", "..", "..", "..", "infra", "postgres", "migrations", name)
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read migration %s: %v", name, err)
	}
	return string(contents)
}

type ledgerDB struct {
	// admin runs the migrations (simple protocol: a migration is a multi-statement file).
	admin *pgxpool.Pool
	// pool is how the service connects: the default protocol, as in production.
	pool   *pgxpool.Pool
	schema string
}

// newLedgerDB makes a schema holding cost_events as the two older migrations leave it. The
// caller applies the migration under test.
func newLedgerDB(t *testing.T) *ledgerDB {
	t.Helper()
	databaseURL := requireTestDatabase(t)
	ctx := context.Background()
	schema := "cost_v2_" + strings.ReplaceAll(uuid.NewString(), "-", "")
	identifier := pgx.Identifier{schema}.Sanitize()

	root, err := pgxpool.ParseConfig(databaseURL)
	if err != nil {
		t.Fatalf("parse TEST_DATABASE_URL: %v", err)
	}
	root.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
	setup, err := pgxpool.NewWithConfig(ctx, root)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	if _, err := setup.Exec(ctx, "CREATE SCHEMA "+identifier); err != nil {
		setup.Close()
		t.Fatalf("create the disposable schema: %v", err)
	}
	t.Cleanup(func() {
		_, _ = setup.Exec(context.Background(), "DROP SCHEMA "+identifier+" CASCADE")
		setup.Close()
	})

	open := func(simple bool) *pgxpool.Pool {
		config, err := pgxpool.ParseConfig(databaseURL)
		if err != nil {
			t.Fatalf("parse: %v", err)
		}
		if simple {
			config.ConnConfig.DefaultQueryExecMode = pgx.QueryExecModeSimpleProtocol
		}
		config.ConnConfig.RuntimeParams["search_path"] = schema
		config.MaxConns = 6
		pool, err := pgxpool.NewWithConfig(ctx, config)
		if err != nil {
			t.Fatalf("connect to the schema: %v", err)
		}
		t.Cleanup(pool.Close)
		return pool
	}
	db := &ledgerDB{admin: open(true), pool: open(false), schema: schema}
	for _, name := range []string{"20260508140000_add_cost_events.sql", "20260508180000_add_user_id_to_cost_events.sql"} {
		if _, err := db.admin.Exec(ctx, readMigration(t, name)); err != nil {
			t.Fatalf("apply %s: %v", name, err)
		}
	}
	return db
}

func (db *ledgerDB) migrate(t *testing.T) {
	t.Helper()
	if _, err := db.admin.Exec(context.Background(), readMigration(t, costEventsMigrationVersion)); err != nil {
		t.Fatalf("apply %s: %v", costEventsMigrationVersion, err)
	}
}

func (db *ledgerDB) count(t *testing.T, where string, args ...any) int {
	t.Helper()
	var n int
	if err := db.pool.QueryRow(context.Background(), "SELECT count(*) FROM cost_events WHERE "+where, args...).Scan(&n); err != nil {
		t.Fatalf("count: %v", err)
	}
	return n
}

func expectConstraint(t *testing.T, err error, constraint string) {
	t.Helper()
	var pgErr *pgconn.PgError
	if !errors.As(err, &pgErr) {
		t.Fatalf("want a violation of %s, got: %v", constraint, err)
	}
	if pgErr.Code != "23514" || pgErr.ConstraintName != constraint {
		t.Fatalf("want 23514 %s, got %s %q: %s", constraint, pgErr.Code, pgErr.ConstraintName, pgErr.Message)
	}
}

// The migration is idempotent and leaves history alone: version 1 rows written before it
// keep their numbers, gain nothing but a feature, are never forwarded, and a second run
// changes nothing.
func TestMigrationLeavesVersionOneRowsAndRunsTwice(t *testing.T) {
	db := newLedgerDB(t)
	ctx := context.Background()
	for _, row := range []struct {
		eventType string
		model     string
		key       string
		user      any
	}{
		{"embedding", "embed-v-4-0", "org-a:old-1", "user-a"},
		{"rerank", "rerank-v3", "org-a:old-2", nil},
		{"extraction", "gpt-4o", "org-a:old-3", nil},
	} {
		if _, err := db.pool.Exec(ctx, `INSERT INTO cost_events (event_type, model, org_id, user_id, count, estimated_tokens, idempotency_key)
			VALUES ($1, $2, 'org-a', $3, 4, 128, $4)`, row.eventType, row.model, row.user, row.key); err != nil {
			t.Fatalf("seed a version 1 row: %v", err)
		}
	}
	db.migrate(t)
	db.migrate(t)

	var version int
	var feature, basis *string
	var input *int64
	var forwarded *time.Time
	for key, want := range map[string]*string{"org-a:old-1": strPtr("embedding"), "org-a:old-2": strPtr("rerank"), "org-a:old-3": nil} {
		if err := db.pool.QueryRow(ctx, `SELECT schema_version, feature, cost_basis, input_tokens, forwarded_at FROM cost_events WHERE idempotency_key = $1`, key).
			Scan(&version, &feature, &basis, &input, &forwarded); err != nil {
			t.Fatalf("read %s: %v", key, err)
		}
		if version != 1 || basis != nil || input != nil || forwarded != nil {
			t.Fatalf("%s: version=%d basis=%v input=%v forwarded=%v: history must say unknown, never zero", key, version, basis, input, forwarded)
		}
		if (feature == nil) != (want == nil) || feature != nil && *feature != *want {
			t.Fatalf("%s: feature = %v, want %v", key, feature, want)
		}
	}
	var tokens int64
	if err := db.pool.QueryRow(ctx, `SELECT estimated_tokens FROM cost_events WHERE idempotency_key = 'org-a:old-1'`).Scan(&tokens); err != nil || tokens != 128 {
		t.Fatalf("a version 1 row's estimated_tokens = %d (%v), want 128 unchanged", tokens, err)
	}
	if n := db.count(t, "forwarded_at IS NULL AND schema_version >= 2"); n != 0 {
		t.Fatalf("history is in the forwarder's queue: %d rows", n)
	}
}

func strPtr(s string) *string { return &s }

// Every rule the table enforces, each on its own row.
func TestTheTableRefusesWhatTheLedgerMustNeverHold(t *testing.T) {
	db := newLedgerDB(t)
	db.migrate(t)
	ctx := context.Background()

	// A complete version 2 row, which every case below spoils in one way.
	valid := map[string]any{
		"event_type": "llm", "model": "gpt-4o-mini", "org_id": "org-a", "count": 2, "estimated_tokens": 70,
		"idempotency_key": "org-a:k", "schema_version": 2, "feature": "contextual-retrieval", "provider_used": "azure-openai",
		"input_tokens": 60, "output_tokens": 10, "cache_read_tokens": 0, "cost_basis": "metered", "estimated": false,
		"zdr": false, "request_id": strings.Repeat("a", 64),
	}
	insert := func(spoil map[string]any) error {
		fields := map[string]any{}
		for k, v := range valid {
			fields[k] = v
		}
		for k, v := range spoil {
			fields[k] = v
		}
		fields["idempotency_key"] = "org-a:" + uuid.NewString()
		var columns, placeholders []string
		var args []any
		for name, value := range fields {
			columns = append(columns, name)
			args = append(args, value)
			placeholders = append(placeholders, fmt.Sprintf("$%d", len(args)))
		}
		_, err := db.pool.Exec(ctx, "INSERT INTO cost_events ("+strings.Join(columns, ", ")+") VALUES ("+strings.Join(placeholders, ", ")+")", args...)
		return err
	}
	if err := insert(nil); err != nil {
		t.Fatalf("a complete version 2 row was refused: %v", err)
	}
	if err := insert(map[string]any{"zdr": true, "user_id": nil, "estimated_tokens": 70}); err != nil {
		t.Fatalf("a zero-retention row without a user was refused: %v", err)
	}
	if err := insert(map[string]any{"cost_basis": "estimated", "estimated": true, "units": 3}); err != nil {
		t.Fatalf("an estimated row with units was refused: %v", err)
	}
	if err := insert(map[string]any{"schema_version": 1, "feature": nil, "provider_used": nil, "input_tokens": nil, "output_tokens": nil,
		"cache_read_tokens": nil, "cost_basis": nil, "estimated": nil, "request_id": nil}); err != nil {
		t.Fatalf("a version 1 row (NULL exact columns) was refused: %v", err)
	}

	for name, tc := range map[string]struct {
		spoil      map[string]any
		constraint string
	}{
		"a zero-retention row that names a user": {map[string]any{"zdr": true, "user_id": "user-a"}, "cost_events_zdr_counters_only_chk"},
		"an unknown schema version":              {map[string]any{"schema_version": 3}, "cost_events_schema_version_chk"},
		"a version 2 row with no feature":        {map[string]any{"feature": nil}, "cost_events_v2_complete_chk"},
		"a version 2 row with no provider":       {map[string]any{"provider_used": nil}, "cost_events_v2_complete_chk"},
		"a version 2 row with no input tokens":   {map[string]any{"input_tokens": nil}, "cost_events_v2_complete_chk"},
		"a version 2 row with no basis":          {map[string]any{"cost_basis": nil}, "cost_events_v2_complete_chk"},
		// Both cost_events_cost_basis_chk and cost_events_v2_complete_chk refuse it; Postgres
		// reports the first of them by name (alphabetical), which is the cost basis one.
		"a version 2 row with no estimated flag": {map[string]any{"estimated": nil}, "cost_events_cost_basis_chk"},
		"a version 2 row with no request id":     {map[string]any{"request_id": nil}, "cost_events_v2_complete_chk"},
		"negative input tokens":                  {map[string]any{"input_tokens": -1}, "cost_events_counts_chk"},
		"negative output tokens":                 {map[string]any{"output_tokens": -1}, "cost_events_counts_chk"},
		"negative cache reads":                   {map[string]any{"cache_read_tokens": -1}, "cost_events_counts_chk"},
		"negative units":                         {map[string]any{"units": -1}, "cost_events_counts_chk"},
		"a negative count":                       {map[string]any{"count": -1}, "cost_events_counts_chk"},
		"an unknown basis":                       {map[string]any{"cost_basis": "subscription"}, "cost_events_cost_basis_chk"},
		"metered but flagged estimated":          {map[string]any{"cost_basis": "metered", "estimated": true}, "cost_events_cost_basis_chk"},
		"estimated but flagged exact":            {map[string]any{"cost_basis": "estimated", "estimated": false}, "cost_events_cost_basis_chk"},
		"a feature in capitals":                  {map[string]any{"feature": "Contextual-Retrieval"}, "cost_events_feature_chk"},
		"a feature with a space":                 {map[string]any{"feature": "graph extraction"}, "cost_events_feature_chk"},
		"a feature that starts with a digit":     {map[string]any{"feature": "1st"}, "cost_events_feature_chk"},
		"a version 1 row marked forwarded":       {map[string]any{"schema_version": 1, "feature": nil, "provider_used": nil, "input_tokens": nil, "output_tokens": nil, "cache_read_tokens": nil, "cost_basis": nil, "estimated": nil, "request_id": nil, "forwarded_at": time.Now()}, "cost_events_forward_state_chk"},
		"a failure class that is free text":      {map[string]any{"forward_error": "dial tcp 10.0.0.4: connection refused"}, "cost_events_forward_state_chk"},
		"negative forward attempts":              {map[string]any{"forward_attempts": -1}, "cost_events_forward_state_chk"},
	} {
		t.Run(name, func(t *testing.T) {
			expectConstraint(t, insert(tc.spoil), tc.constraint)
		})
	}
}

// The consumer's real statement, over the real table: every column lands where it should, the
// organization is in the stored key, a zero-retention row has no user, and the same key twice is
// one row.
func TestTheConsumersStatementRecordsEveryFixtureEnvelopeExactlyOnce(t *testing.T) {
	db := newLedgerDB(t)
	db.migrate(t)
	ctx := context.Background()
	fx := loadFixture(t)
	consumer := NewSignedConsumer(db.pool, nil, fx.registry(t, 10*time.Minute))
	for pass := 1; pass <= 2; pass++ {
		for _, tc := range fx.Cases {
			// A second delivery after a restart: a new process, an empty replay cache.
			consumer.verifier = fx.registry(t, 10*time.Minute)
			if _, err := consumer.handle(ctx, []byte(tc.Envelope), true); err != nil {
				t.Fatalf("pass %d, %s: %v", pass, tc.Name, err)
			}
		}
	}
	if n := db.count(t, "true"); n != len(fx.Cases) {
		t.Fatalf("%d rows for %d events delivered twice: a duplicate was recorded", n, len(fx.Cases))
	}
	for _, tc := range fx.Cases {
		want := tc.Expect
		var (
			org, eventType, feature, provider, model, basis, request, key string
			user                                                          *string
			count                                                         int
			estimatedTokens, input, output, cacheRead                     int64
			units                                                         *int64
			estimated, zdr                                                bool
			version                                                       int
		)
		err := db.pool.QueryRow(ctx, `SELECT org_id, event_type, feature, provider_used, model, cost_basis, request_id, idempotency_key,
			user_id, count, estimated_tokens, input_tokens, output_tokens, cache_read_tokens, units, estimated, zdr, schema_version
			FROM cost_events WHERE idempotency_key = $1`, want.OrgID+":"+want.IdempotencyKey).
			Scan(&org, &eventType, &feature, &provider, &model, &basis, &request, &key, &user, &count, &estimatedTokens, &input, &output, &cacheRead, &units, &estimated, &zdr, &version)
		if err != nil {
			t.Fatalf("%s: read back: %v", tc.Name, err)
		}
		if org != want.OrgID || eventType != want.EventType || feature != want.Feature || provider != want.ProviderUsed || model != want.ModelUsed ||
			basis != want.CostBasis || request != want.RequestDigest || count != want.Count || estimatedTokens != want.InputTokens+want.OutputTokens ||
			input != want.InputTokens || output != want.OutputTokens || cacheRead != want.CacheReadTokens || estimated != want.Estimated ||
			zdr != want.ZDR || version != 2 || (units == nil) != (want.Units == nil) || units != nil && *units != *want.Units {
			t.Fatalf("%s: row does not match what was signed", tc.Name)
		}
		if user != nil {
			t.Fatalf("%s: a usage row names user %q", tc.Name, *user)
		}
	}
	if n := db.count(t, "zdr AND user_id IS NULL"); n != 2 {
		t.Fatalf("%d zero-retention rows, want the 2 of the fixture, recorded as counters only", n)
	}
}

// The forwarder's statements: due rows are leased, replicas never share one, a failure is
// remembered with when to retry, success removes the row from the queue, history is never queued.
func TestTheForwarderQueueStatementsLeaseRetryAndNeverQueueHistory(t *testing.T) {
	db := newLedgerDB(t)
	db.migrate(t)
	ctx := context.Background()
	insertV2 := func(org, seed string) int64 {
		var id int64
		err := db.pool.QueryRow(ctx, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key,
			schema_version, feature, provider_used, input_tokens, output_tokens, cache_read_tokens, cost_basis, estimated, zdr, request_id)
			VALUES ('llm', 'gpt-4o-mini', $1::text, 1, 70, $1::text || ':usage:graph-extraction:' || $2::text, 2, 'graph-extraction', 'azure-openai', 60, 10, 5, 'metered', false, false, $2::text)
			RETURNING id`, org, seed).Scan(&id)
		if err != nil {
			t.Fatalf("insert: %v", err)
		}
		return id
	}
	for i := 0; i < 6; i++ {
		insertV2("org-a", fmt.Sprintf("%064d", i))
	}
	if _, err := db.pool.Exec(ctx, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens) VALUES ('embedding', 'old', 'org-a', 1, 1)`); err != nil {
		t.Fatalf("seed history: %v", err)
	}

	storeA, storeB := NewPGForwardStore(db.pool), NewPGForwardStore(db.pool)

	// Two replicas claiming at once get disjoint rows, and history is not among them.
	first, err := storeA.Claim(ctx, 4, time.Minute)
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	second, err := storeB.Claim(ctx, 4, time.Minute)
	if err != nil {
		t.Fatalf("claim: %v", err)
	}
	if len(first) != 4 || len(second) != 2 {
		t.Fatalf("claimed %d and %d rows of 6", len(first), len(second))
	}
	seen := map[int64]bool{}
	for _, row := range append(append([]PendingRow{}, first...), second...) {
		if seen[row.ID] {
			t.Fatalf("row %d was claimed by two replicas", row.ID)
		}
		seen[row.ID] = true
	}
	for i := 1; i < len(first); i++ {
		if first[i].ID <= first[i-1].ID {
			t.Fatal("rows are not oldest first")
		}
	}
	row := first[0]
	if row.OrgID != "org-a" || row.Feature != "graph-extraction" || row.ProviderUsed != "azure-openai" || row.Model != "gpt-4o-mini" ||
		row.InputTokens != 60 || row.OutputTokens != 10 || row.CacheReadTokens != 5 || row.CostBasis != "metered" || row.Estimated ||
		!strings.HasPrefix(row.IdempotencyKey, "org-a:usage:graph-extraction:") || len(row.RequestID) != 64 || row.Attempts != 0 {
		t.Fatalf("a claimed row lost a field: %+v", row)
	}
	// While leased, nothing is claimable.
	if rows, _ := storeA.Claim(ctx, 10, time.Minute); len(rows) != 0 {
		t.Fatalf("leased rows were claimed again: %d", len(rows))
	}

	// A failure is remembered, with when to try again; the lease is released.
	next := time.Now().Add(time.Hour)
	if err := storeA.MarkFailed(ctx, first[0].ID, "unavailable", next); err != nil {
		t.Fatalf("mark failed: %v", err)
	}
	var attempts int
	var class *string
	var lease *time.Time
	if err := db.pool.QueryRow(ctx, `SELECT forward_attempts, forward_error, forward_lease_until FROM cost_events WHERE id = $1`, first[0].ID).Scan(&attempts, &class, &lease); err != nil {
		t.Fatalf("read: %v", err)
	}
	if attempts != 1 || class == nil || *class != "unavailable" || lease != nil {
		t.Fatalf("after a failure: attempts=%d class=%v lease=%v", attempts, class, lease)
	}
	// Success removes the row from the queue for good.
	if err := storeA.MarkForwarded(ctx, first[1].ID); err != nil {
		t.Fatalf("mark forwarded: %v", err)
	}
	if err := storeA.MarkFailed(ctx, first[1].ID, "rejected", time.Now()); err != nil {
		t.Fatalf("a late failure of a forwarded row: %v", err)
	}
	if n := db.count(t, "id = $1 AND forwarded_at IS NOT NULL AND forward_attempts = 0", first[1].ID); n != 1 {
		t.Fatal("a failure was recorded against a row that was already forwarded")
	}

	// Leases end: the rest become claimable again; the failed one waits for its time and the
	// forwarded one never returns.
	if _, err := db.pool.Exec(ctx, `UPDATE cost_events SET forward_lease_until = now() - interval '1 second' WHERE forward_lease_until IS NOT NULL`); err != nil {
		t.Fatalf("expire the leases: %v", err)
	}
	again, err := storeB.Claim(ctx, 10, time.Minute)
	if err != nil {
		t.Fatalf("claim after the leases ended: %v", err)
	}
	if len(again) != 4 {
		t.Fatalf("claimed %d rows, want the 4 that are neither forwarded (1) nor waiting for a retry (1)", len(again))
	}
	for _, r := range again {
		if r.ID == first[0].ID || r.ID == first[1].ID {
			t.Fatalf("row %d was claimed although it is forwarded or not yet due", r.ID)
		}
	}
	if _, err := db.pool.Exec(ctx, `UPDATE cost_events SET forward_next_at = now() - interval '1 second' WHERE id = $1`, first[0].ID); err != nil {
		t.Fatalf("make the failed row due: %v", err)
	}
	if rows, _ := storeA.Claim(ctx, 10, time.Minute); len(rows) != 1 || rows[0].ID != first[0].ID || rows[0].Attempts != 1 {
		t.Fatalf("the due retry was not claimed with its attempt count: %+v", rows)
	}

	pending, oldest, err := storeA.Backlog(ctx)
	if err != nil || pending != 5 || oldest < 0 {
		t.Fatalf("backlog = %d, %v (%v), want 5 rows pending: the forwarded one and history are not counted", pending, oldest, err)
	}
}
