package postgrescontract

import (
	"context"
	"fmt"
	"regexp"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Everything in this file runs against a real Postgres 16: the real init.sql, every real
// migration applied by the real tools/migrator, and, for the two statements the Go services
// run, the statements themselves, read out of the Go source (no copy to drift). Without
// TEST_DATABASE_URL the tests skip locally and FAIL under REQUIRE_CONTRACT or CI.

// costEventLines are the catalog entries of cost_events: its columns, constraints and
// indexes, with comments left out (init.sql's mirror carries none).
func costEventLines(t *testing.T, pool *pgxpool.Pool) []string {
	t.Helper()
	var out []string
	for _, line := range catalog(t, pool) {
		isCostEvents := strings.HasPrefix(line, "col:cost_events.") || strings.HasPrefix(line, "con:cost_events.") ||
			(strings.HasPrefix(line, "idx:") && strings.Contains(line, " ON public.cost_events "))
		if isCostEvents {
			out = append(out, line)
		}
	}
	return out
}

var costKeySeq atomic.Int64

func goConstant(t *testing.T, relative, name string) string {
	t.Helper()
	source := readSource(t, relative)
	m := regexp.MustCompile("(?s)" + name + " = `(.*?)`").FindStringSubmatch(source)
	if m == nil {
		t.Fatalf("cannot find %s in %s", name, relative)
	}
	return m[1]
}

// init.sql is the baseline of a fresh database and carries a mirror of the migration. Applied
// to a fresh database the migrator must find nothing left to change: the mirror is the
// migration, as the database sees them. The rollback takes the table back to what it was
// before, rows included, and the migration then applies again to a table that has history in it.
func TestCostEventsMigrationMirrorLifecycleAndHistory(t *testing.T) {
	db := newScratchDatabase(t)
	pool := openPool(t, db)

	mirrored := costEventLines(t, pool) // what init.sql alone builds
	migrate(t, db, "up")
	migrated := costEventLines(t, pool)
	if missing, extra := diffSets(mirrored, migrated); len(missing)+len(extra) > 0 {
		t.Fatalf("init.sql's mirror and the migration disagree about cost_events\n  only in init.sql (%d): %v\n  only in the migrated table (%d): %v", len(missing), missing, len(extra), extra)
	}
	for _, column := range []string{"schema_version", "feature", "provider_used", "input_tokens", "output_tokens", "cache_read_tokens",
		"units", "cost_basis", "estimated", "zdr", "request_id", "forwarded_at", "forward_attempts", "forward_next_at", "forward_lease_until", "forward_error"} {
		if !hasEntry(migrated, "col:cost_events."+column+":") {
			t.Errorf("cost_events has no column %s after the migration", column)
		}
	}
	whole := catalog(t, pool)

	// Down to the version before: the table is what it was, and rows that existed survive.
	execSQL(t, pool, `INSERT INTO cost_events (event_type, model, org_id, user_id, count, estimated_tokens, idempotency_key)
		VALUES ('embedding', 'embed-v-4-0', 'org-a', 'user-a', 2, 40, 'org-a:v1-a'), ('rerank', 'rerank-v3', 'org-a', NULL, 5, 160, 'org-a:v1-b'),
		       ('extraction', 'gpt-4o', 'org-a', NULL, 1, 9, 'org-a:v1-c')`)
	execSQL(t, pool, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, schema_version, feature, provider_used,
		input_tokens, output_tokens, cache_read_tokens, cost_basis, estimated, zdr, request_id)
		VALUES ('llm', 'gpt-4o-mini', 'org-a', 1, 70, 'org-a:v2-a', 2, 'contextual-retrieval', 'azure-openai', 60, 10, 0, 'metered', false, false, repeat('a', 64))`)
	migrate(t, db, "down", "--to", cprVersion)
	if got := costEventLines(t, pool); hasEntry(got, "col:cost_events.feature:") || hasEntry(got, "col:cost_events.forwarded_at:") {
		t.Fatalf("the rollback left v2 columns behind: %v", got)
	}
	if hasEntry(catalog(t, pool), "con:cost_events.cost_events_zdr_counters_only_chk") {
		t.Fatal("the rollback left a constraint behind")
	}
	if n := queryInt(t, pool, `SELECT count(*) FROM cost_events`); n != 4 {
		t.Fatalf("the rollback changed the number of rows: %d, want 4 (it drops what the migration added, never rows)", n)
	}
	if n := queryInt(t, pool, `SELECT estimated_tokens FROM cost_events WHERE idempotency_key = 'org-a:v1-a'`); n != 40 {
		t.Fatalf("a row's own number changed on the way down: %d", n)
	}

	// Up again, over a table that has history in it.
	migrate(t, db, "up")
	assertSameCatalog(t, "up, down, up", whole, catalog(t, pool))
	if got := queryString(t, pool, `SELECT feature FROM cost_events WHERE idempotency_key = 'org-a:v1-a'`); got != "embedding" {
		t.Errorf("an embedding row of history was not given the feature embedding: %s", got)
	}
	if got := queryString(t, pool, `SELECT feature FROM cost_events WHERE idempotency_key = 'org-a:v1-b'`); got != "rerank" {
		t.Errorf("a rerank row of history was not given the feature rerank: %s", got)
	}
	if got := queryString(t, pool, `SELECT feature FROM cost_events WHERE idempotency_key = 'org-a:v1-c'`); got != "<NULL>" {
		t.Errorf("a row of an unknown kind was given the feature %s: only the two kinds the table was built for are", got)
	}
	// Everything v1 says unknown, nothing is in the forwarder's queue, and v2 numbers are NULL after a round trip.
	if n := queryInt(t, pool, `SELECT count(*) FROM cost_events WHERE schema_version = 1 AND (input_tokens IS NOT NULL OR cost_basis IS NOT NULL OR estimated IS NOT NULL OR forwarded_at IS NOT NULL)`); n != 0 {
		t.Errorf("%d rows of history carry exact numbers or forwarding state", n)
	}
	if n := queryInt(t, pool, `SELECT count(*) FROM cost_events WHERE forwarded_at IS NULL AND schema_version >= 2`); n != 0 {
		t.Errorf("history is in the forwarder's queue: %d rows (the rollback dropped the v2 row's columns, so it reads as version 1 again)", n)
	}

	// A second application of the same file changes nothing.
	execSQL(t, pool, readSQL(t, cevUp))
	assertSameCatalog(t, "the migration applied twice", whole, catalog(t, pool))
}

// Every rule the table enforces, on rows written the way the consumer writes them.
func TestCostEventsTableRefusesWhatTheLedgerMustNeverHold(t *testing.T) {
	db := newScratchDatabase(t)
	migrate(t, db, "up")
	pool := openPool(t, db)
	ctx := context.Background()

	insert := func(spoil map[string]string) error {
		fields := map[string]string{
			"event_type": "'llm'", "model": "'gpt-4o-mini'", "org_id": "'org-a'", "count": "2", "estimated_tokens": "70",
			"schema_version": "2", "feature": "'contextual-retrieval'", "provider_used": "'azure-openai'",
			"input_tokens": "60", "output_tokens": "10", "cache_read_tokens": "0", "cost_basis": "'metered'", "estimated": "false",
			"zdr": "false", "request_id": "repeat('a', 64)",
		}
		for k, v := range spoil {
			fields[k] = v
		}
		fields["idempotency_key"] = fmt.Sprintf("'org-a:row-%d'", costKeySeq.Add(1))
		var columns, values []string
		for k, v := range fields {
			columns, values = append(columns, k), append(values, v)
		}
		_, err := pool.Exec(ctx, "INSERT INTO cost_events ("+strings.Join(columns, ", ")+") VALUES ("+strings.Join(values, ", ")+")")
		return err
	}
	v1 := map[string]string{"schema_version": "1", "feature": "NULL", "provider_used": "NULL", "input_tokens": "NULL", "output_tokens": "NULL",
		"cache_read_tokens": "NULL", "cost_basis": "NULL", "estimated": "NULL", "request_id": "NULL"}
	spoilV1 := func(extra map[string]string) map[string]string {
		out := map[string]string{}
		for k, v := range v1 {
			out[k] = v
		}
		for k, v := range extra {
			out[k] = v
		}
		return out
	}

	for name, row := range map[string]map[string]string{
		"a complete version 2 row":                  nil,
		"a zero-retention row without a user":       {"zdr": "true"},
		"an estimated row with units":               {"cost_basis": "'estimated'", "estimated": "true", "units": "3"},
		"a version 1 row with NULL exact columns":   v1,
		"an old consumer's row (the seven columns)": {"schema_version": "1", "feature": "NULL", "provider_used": "NULL", "input_tokens": "NULL", "output_tokens": "NULL", "cache_read_tokens": "NULL", "cost_basis": "NULL", "estimated": "NULL", "request_id": "NULL", "zdr": "false"},
	} {
		if err := insert(row); err != nil {
			t.Fatalf("%s was refused: %v", name, err)
		}
	}

	for name, tc := range map[string]struct {
		row        map[string]string
		constraint string
	}{
		"a zero-retention row that names a user": {map[string]string{"zdr": "true", "user_id": "'user-a'"}, "cost_events_zdr_counters_only_chk"},
		"an unknown schema version":              {map[string]string{"schema_version": "3"}, "cost_events_schema_version_chk"},
		"a version 2 row with no feature":        {map[string]string{"feature": "NULL"}, "cost_events_v2_complete_chk"},
		"a version 2 row with no provider":       {map[string]string{"provider_used": "NULL"}, "cost_events_v2_complete_chk"},
		"a version 2 row with no input tokens":   {map[string]string{"input_tokens": "NULL"}, "cost_events_v2_complete_chk"},
		"a version 2 row with no output tokens":  {map[string]string{"output_tokens": "NULL"}, "cost_events_v2_complete_chk"},
		"a version 2 row with no cache reads":    {map[string]string{"cache_read_tokens": "NULL"}, "cost_events_v2_complete_chk"},
		"a version 2 row with no basis":          {map[string]string{"cost_basis": "NULL"}, "cost_events_v2_complete_chk"},
		// Both cost_events_cost_basis_chk and cost_events_v2_complete_chk refuse it; Postgres
		// reports the first of them by name (alphabetical), which is the cost basis one.
		"a version 2 row with no estimated flag": {map[string]string{"estimated": "NULL"}, "cost_events_cost_basis_chk"},
		"a version 2 row with no request id":     {map[string]string{"request_id": "NULL"}, "cost_events_v2_complete_chk"},
		"negative input tokens":                  {map[string]string{"input_tokens": "-1"}, "cost_events_counts_chk"},
		"negative output tokens":                 {map[string]string{"output_tokens": "-1"}, "cost_events_counts_chk"},
		"negative cache reads":                   {map[string]string{"cache_read_tokens": "-1"}, "cost_events_counts_chk"},
		"negative units":                         {map[string]string{"units": "-1"}, "cost_events_counts_chk"},
		"a negative count":                       {map[string]string{"count": "-1"}, "cost_events_counts_chk"},
		"a negative estimate":                    {map[string]string{"estimated_tokens": "-1"}, "cost_events_counts_chk"},
		"an unknown basis":                       {map[string]string{"cost_basis": "'subscription'"}, "cost_events_cost_basis_chk"},
		"metered but flagged estimated":          {map[string]string{"cost_basis": "'metered'", "estimated": "true"}, "cost_events_cost_basis_chk"},
		"estimated but flagged exact":            {map[string]string{"cost_basis": "'estimated'", "estimated": "false"}, "cost_events_cost_basis_chk"},
		"a feature in capitals":                  {map[string]string{"feature": "'Contextual-Retrieval'"}, "cost_events_feature_chk"},
		"a feature with a space":                 {map[string]string{"feature": "'graph extraction'"}, "cost_events_feature_chk"},
		"a feature that starts with a digit":     {map[string]string{"feature": "'1st'"}, "cost_events_feature_chk"},
		"a feature of 64 characters":             {map[string]string{"feature": "'a" + strings.Repeat("b", 63) + "'"}, "cost_events_feature_chk"},
		"a version 1 row marked forwarded":       {spoilV1(map[string]string{"forwarded_at": "now()"}), "cost_events_forward_state_chk"},
		"a failure class that is free text":      {map[string]string{"forward_error": "'dial tcp 10.0.0.4: connection refused'"}, "cost_events_forward_state_chk"},
		"negative forward attempts":              {map[string]string{"forward_attempts": "-1"}, "cost_events_forward_state_chk"},
	} {
		t.Run(name, func(t *testing.T) {
			expectPgError(t, insert(tc.row), "23514", tc.constraint)
		})
	}
	// And a valid failure class and a forwarded version 2 row.
	for _, class := range []string{"rejected", "unauthorized", "unavailable", "token_unavailable"} {
		if err := insert(map[string]string{"forward_error": "'" + class + "'", "forward_attempts": "1"}); err != nil {
			t.Errorf("failure class %s was refused: %v", class, err)
		}
	}
	if err := insert(map[string]string{"forwarded_at": "now()"}); err != nil {
		t.Errorf("a forwarded version 2 row was refused: %v", err)
	}
}

// The consumer's statement, read out of consumer.go, over the real table: version 1 and version
// 2 values both land, the zero-retention row has no user, and the same key twice is one row.
func TestTheConsumersRealStatementWritesBothVersionsAndIgnoresADuplicate(t *testing.T) {
	db := newScratchDatabase(t)
	migrate(t, db, "up")
	pool := openPool(t, db)
	ctx := context.Background()
	statement := goConstant(t, "services/data-orchestrator-go/internal/cost/consumer.go", "insertCostEventSQL")

	digest := strings.Repeat("b", 64)
	v2 := []any{"llm", "gpt-4o-mini", "org-a", nil, 3, int64(1272), "org-a:usage:contextual-retrieval:" + digest,
		2, "contextual-retrieval", "azure-openai", int64(1200), int64(72), int64(512), nil, "metered", false, false, digest}
	v1 := []any{"embedding", "embed-v-4-0", "org-a", "user-a", 24, int64(9100), "org-a:embed.cost-0123456789abcdef",
		1, "embedding", nil, nil, nil, nil, nil, nil, nil, false, nil}
	zdr := []any{"llm", "gpt-4o", "org-z", nil, 1, int64(780), "org-z:usage:graph-extraction:" + digest,
		2, "graph-extraction", "azure-openai", int64(700), int64(80), int64(0), nil, "metered", false, true, digest}
	for _, row := range [][]any{v2, v1, zdr, v2, v1, zdr} {
		if _, err := pool.Exec(ctx, statement, row...); err != nil {
			t.Fatalf("the consumer's statement failed for %v: %v", row[6], err)
		}
	}
	if n := queryInt(t, pool, `SELECT count(*) FROM cost_events`); n != 3 {
		t.Fatalf("%d rows for three events each written twice, want 3", n)
	}
	if got := queryString(t, pool, `SELECT input_tokens || '/' || output_tokens || '/' || cache_read_tokens || '/' || cost_basis || '/' || feature FROM cost_events WHERE org_id = 'org-a' AND event_type = 'llm'`); got != "1200/72/512/metered/contextual-retrieval" {
		t.Errorf("the version 2 row reads back %s", got)
	}
	if got := queryString(t, pool, `SELECT coalesce(input_tokens::text, 'NULL') || '/' || coalesce(cost_basis, 'NULL') || '/' || feature || '/' || schema_version FROM cost_events WHERE event_type = 'embedding'`); got != "NULL/NULL/embedding/1" {
		t.Errorf("the version 1 row reads back %s: unknown must stay NULL", got)
	}
	if got := queryString(t, pool, `SELECT zdr::text || '/' || coalesce(user_id, 'NULL') FROM cost_events WHERE org_id = 'org-z'`); got != "true/NULL" {
		t.Errorf("the zero-retention row reads back %s", got)
	}
}

// The forwarder's statements, read out of forwarder.go, over the real table: due rows are
// leased oldest first, two connections at once never get the same row, a failure is
// remembered, success removes a row from the queue, and history is never queued.
func TestTheForwardersRealStatementsLeaseFailAndForward(t *testing.T) {
	db := newScratchDatabase(t)
	migrate(t, db, "up")
	pool := openPool(t, db)
	ctx := context.Background()
	const path = "services/data-orchestrator-go/internal/cost/forwarder.go"
	claim := goConstant(t, path, "claimUsageRowsSQL")
	markForwarded := goConstant(t, path, "markForwardedSQL")
	markFailed := goConstant(t, path, "markFailedSQL")
	backlog := goConstant(t, path, "backlogSQL")

	for i := 0; i < 6; i++ {
		execSQL(t, pool, fmt.Sprintf(`INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, schema_version, feature, provider_used,
			input_tokens, output_tokens, cache_read_tokens, cost_basis, estimated, zdr, request_id)
			VALUES ('llm', 'gpt-4o-mini', 'org-a', 1, 70, 'org-a:k%d', 2, 'graph-extraction', 'azure-openai', 60, 10, 5, 'metered', false, false, repeat('c', 64))`, i))
	}
	execSQL(t, pool, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens) VALUES ('embedding', 'old', 'org-a', 1, 1)`)

	claimRows := func(limit int) []int64 {
		rows, err := pool.Query(ctx, claim, limit, 60.0)
		if err != nil {
			t.Fatalf("claim: %v", err)
		}
		defer rows.Close()
		var ids []int64
		for rows.Next() {
			values, err := rows.Values()
			if err != nil {
				t.Fatalf("claim row: %v", err)
			}
			ids = append(ids, values[0].(int64))
		}
		if err := rows.Err(); err != nil {
			t.Fatalf("claim rows: %v", err)
		}
		return ids
	}
	first, second := claimRows(4), claimRows(4)
	if len(first) != 4 || len(second) != 2 {
		t.Fatalf("claimed %d then %d rows of 6 (history excluded)", len(first), len(second))
	}
	seen := map[int64]bool{}
	for _, id := range append(append([]int64{}, first...), second...) {
		if seen[id] {
			t.Fatalf("row %d was claimed twice while leased", id)
		}
		seen[id] = true
	}
	if len(claimRows(10)) != 0 {
		t.Fatal("leased rows were claimed again")
	}

	if _, err := pool.Exec(ctx, markFailed, first[0], "unavailable", time.Now().Add(time.Hour)); err != nil {
		t.Fatalf("mark failed: %v", err)
	}
	if got := queryString(t, pool, fmt.Sprintf(`SELECT forward_attempts || '/' || forward_error || '/' || coalesce(forward_lease_until::text, 'NULL') FROM cost_events WHERE id = %d`, first[0])); got != "1/unavailable/NULL" {
		t.Fatalf("a failed forward reads back %s", got)
	}
	if _, err := pool.Exec(ctx, markForwarded, first[1]); err != nil {
		t.Fatalf("mark forwarded: %v", err)
	}
	if _, err := pool.Exec(ctx, markFailed, first[1], "rejected", time.Now()); err != nil {
		t.Fatalf("a late failure: %v", err)
	}
	if got := queryString(t, pool, fmt.Sprintf(`SELECT (forwarded_at IS NOT NULL)::text || '/' || forward_attempts FROM cost_events WHERE id = %d`, first[1])); got != "true/0" {
		t.Fatalf("a failure was recorded against a forwarded row: %s", got)
	}

	execSQL(t, pool, `UPDATE cost_events SET forward_lease_until = now() - interval '1 second' WHERE forward_lease_until IS NOT NULL`)
	if again := claimRows(10); len(again) != 4 {
		t.Fatalf("after the leases ended %d rows were claimable, want 4 (one forwarded, one waiting for its retry time)", len(again))
	}

	var pending int64
	var oldest float64
	if err := pool.QueryRow(ctx, backlog).Scan(&pending, &oldest); err != nil || pending != 5 || oldest < 0 {
		t.Fatalf("backlog = %d, %v, %v: want 5 pending (the forwarded row and history are not counted)", pending, oldest, err)
	}
}

// The claim names every condition of the partial index, so Postgres can use it; checked
// against the planner on a table big enough that a sequential scan is not the cheap plan.
func TestTheClaimUsesTheForwardQueueIndex(t *testing.T) {
	db := newScratchDatabase(t)
	migrate(t, db, "up")
	pool := openPool(t, db)
	execSQL(t, pool, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key)
		SELECT 'embedding', 'old', 'org-a', 1, 1, 'org-a:h' || g FROM generate_series(1, 20000) g`)
	execSQL(t, pool, `INSERT INTO cost_events (event_type, model, org_id, count, estimated_tokens, idempotency_key, schema_version, feature, provider_used,
		input_tokens, output_tokens, cache_read_tokens, cost_basis, estimated, zdr, request_id)
		VALUES ('llm', 'gpt-4o-mini', 'org-a', 1, 70, 'org-a:waiting', 2, 'graph-extraction', 'azure-openai', 60, 10, 5, 'metered', false, false, repeat('c', 64))`)
	execSQL(t, pool, `ANALYZE cost_events`)
	lines := queryLines(t, pool, `EXPLAIN SELECT id FROM cost_events
		 WHERE forwarded_at IS NULL AND schema_version >= 2
		   AND (forward_next_at IS NULL OR forward_next_at <= now())
		   AND (forward_lease_until IS NULL OR forward_lease_until < now())
		 ORDER BY id LIMIT 50`)
	if plan := strings.Join(lines, "\n"); !strings.Contains(plan, "idx_cost_events_forward_queue") {
		t.Fatalf("the claim does not use idx_cost_events_forward_queue:\n%s", plan)
	}
}
