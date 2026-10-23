package store_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/prometheus/client_golang/prometheus"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/events"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// execMaintenance runs one statement in a transaction that lifts the
// audit_events append-only guard (migration 007), for test setup that has to
// backdate, tamper with or delete rows.
func execMaintenance(ctx context.Context, pool *pgxpool.Pool, sql string, args ...any) error {
	tx, err := pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer func() { _ = tx.Rollback(ctx) }() // no-op after a successful Commit
	if _, err := tx.Exec(ctx, `SELECT set_config('audit.maintenance', 'on', true)`); err != nil {
		return err
	}
	if _, err := tx.Exec(ctx, sql, args...); err != nil {
		return err
	}
	return tx.Commit(ctx)
}

// chainDB connects to TEST_DATABASE_URL and applies the migrations, or skips.
func chainDB(t *testing.T) (context.Context, *pgxpool.Pool) {
	t.Helper()
	dsn := os.Getenv("TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("TEST_DATABASE_URL not set; skipping Postgres integration test")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
	t.Cleanup(cancel)
	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("connect: %v", err)
	}
	t.Cleanup(pool.Close)
	if err := store.Migrate(ctx, pool); err != nil {
		t.Fatalf("migrate: %v", err)
	}
	return ctx, pool
}

// chainOrg returns a unique org id and removes its rows and head afterwards.
func chainOrg(t *testing.T, pool *pgxpool.Pool, label string) string {
	t.Helper()
	org := fmt.Sprintf("org-chain-%s-%d", label, time.Now().UnixNano())
	t.Cleanup(func() {
		ctx := context.Background()
		_ = execMaintenance(ctx, pool, `DELETE FROM audit_events WHERE org_id = $1`, org)
		_, _ = pool.Exec(ctx, `DELETE FROM audit_chain_heads WHERE org_id = $1`, org)
	})
	return org
}

func chainEvent(org string, n int) *events.AuditEvent {
	return &events.AuditEvent{
		EventID:    fmt.Sprintf("chain:%s:%d", org, n),
		OccurredAt: time.Now().UTC(),
		OrgID:      org,
		UserID:     "user-1",
		Plane:      "control",
		Producer:   "audit-core-test",
		Event:      "chain_test",
		Details:    map[string]any{"n": n, "tool": "knowledge_search"},
	}
}

type chainedRow struct {
	seq     int64
	prev    []byte
	hash    []byte
	mode    string
	version int16
	id      int64
	row     chain.Row
}

// loadChain reads an org's chained rows in chain order, exactly the columns a
// verifier would hash.
func loadChain(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string) []chainedRow {
	t.Helper()
	rows, err := pool.Query(ctx, `
		SELECT chain_seq, prev_hash, row_hash, chain_mode, chain_version, id, org_id,
		       occurred_at, ingested_at, COALESCE(user_id, ''), COALESCE(actor_role, ''),
		       plane, event, COALESCE(subject, ''), COALESCE(resource_id, ''), outcome,
		       COALESCE(event_id, ''), COALESCE(request_id, ''), COALESCE(host(ip_address), ''),
		       COALESCE(user_agent, ''), COALESCE(source_producer, ''), COALESCE(source_bus, ''),
		       payload_hash, details::text
		FROM audit_events
		WHERE org_id = $1 AND chain_seq IS NOT NULL
		ORDER BY chain_seq`, org)
	if err != nil {
		t.Fatalf("load chain: %v", err)
	}
	defer rows.Close()
	var out []chainedRow
	for rows.Next() {
		var c chainedRow
		r := &c.row
		if err := rows.Scan(&c.seq, &c.prev, &c.hash, &c.mode, &c.version, &c.id, &r.OrgID,
			&r.OccurredAt, &r.IngestedAt, &r.UserID, &r.ActorRole,
			&r.Plane, &r.Event, &r.Subject, &r.ResourceID, &r.Outcome,
			&r.EventID, &r.RequestID, &r.Host,
			&r.UserAgent, &r.SourceProducer, &r.SourceBus,
			&r.PayloadHash, &r.Details); err != nil {
			t.Fatalf("scan chain row: %v", err)
		}
		r.ChainSeq = c.seq
		out = append(out, c)
	}
	if err := rows.Err(); err != nil {
		t.Fatal(err)
	}
	return out
}

// assertChainValid checks the chain properties S4's verifier will rely on:
// contiguous from 1, genesis first, each prev_hash the previous row_hash, and
// every stored hash equal to a recomputation from the stored columns.
func assertChainValid(t *testing.T, org string, rows []chainedRow) {
	t.Helper()
	prev, err := chain.GenesisHash(org)
	if err != nil {
		t.Fatal(err)
	}
	for i, c := range rows {
		if c.seq != int64(i+1) {
			t.Fatalf("org %s: row %d has chain_seq %d, want %d (gap or reorder)", org, i, c.seq, i+1)
		}
		if !bytes.Equal(c.prev, prev) {
			t.Fatalf("org %s seq %d: prev_hash does not link to the previous row", org, c.seq)
		}
		want, err := chain.RowHash(prev, c.row)
		if err != nil {
			t.Fatalf("org %s seq %d: recompute: %v", org, c.seq, err)
		}
		if !bytes.Equal(c.hash, want) {
			t.Fatalf("org %s seq %d: stored row_hash differs from a recomputation", org, c.seq)
		}
		if c.version != chain.Version {
			t.Fatalf("org %s seq %d: chain_version %d", org, c.seq, c.version)
		}
		prev = c.hash
	}
}

type headRow struct {
	seq   int64
	hash  []byte
	state string
}

func loadHead(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string) (headRow, bool) {
	t.Helper()
	var h headRow
	err := pool.QueryRow(ctx,
		`SELECT head_seq, head_hash, state FROM audit_chain_heads WHERE org_id = $1`, org).
		Scan(&h.seq, &h.hash, &h.state)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return headRow{}, false
		}
		t.Fatalf("load head: %v", err)
	}
	return h, true
}

func assertHeadIsTip(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string, rows []chainedRow) {
	t.Helper()
	head, ok := loadHead(t, ctx, pool, org)
	if !ok {
		t.Fatalf("org %s has no chain head", org)
	}
	if len(rows) == 0 {
		return
	}
	last := rows[len(rows)-1]
	if head.seq != last.seq || !bytes.Equal(head.hash, last.hash) {
		t.Fatalf("org %s: head (seq %d) is not the last row (seq %d)", org, head.seq, last.seq)
	}
}

func counterValue(t *testing.T, name string, labels map[string]string) float64 {
	t.Helper()
	families, err := prometheus.DefaultGatherer.Gather()
	if err != nil {
		t.Fatal(err)
	}
	for _, family := range families {
		if family.GetName() != name {
			continue
		}
		for _, metric := range family.GetMetric() {
			matched := 0
			for _, label := range metric.GetLabel() {
				if want, ok := labels[label.GetName()]; ok && want == label.GetValue() {
					matched++
				}
			}
			if matched == len(labels) {
				return metric.GetCounter().GetValue()
			}
		}
	}
	return 0
}

func TestChainedInsertIsContiguousAndLinked(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "contiguous")
	st := store.New(pool, store.WithChain())

	for n := 1; n <= 5; n++ {
		if _, err := st.InsertAudit(ctx, chainEvent(org, n)); err != nil {
			t.Fatalf("insert %d: %v", n, err)
		}
	}
	rows := loadChain(t, ctx, pool, org)
	if len(rows) != 5 {
		t.Fatalf("chained rows = %d, want 5", len(rows))
	}
	assertChainValid(t, org, rows)
	assertHeadIsTip(t, ctx, pool, org, rows)
	for _, c := range rows {
		if c.mode != "insert" {
			t.Fatalf("seq %d chain_mode = %q, want insert", c.seq, c.mode)
		}
	}
}

func TestChainedInsertFiftyWritersThreeOrgsStayGapFree(t *testing.T) {
	ctx, pool := chainDB(t)
	st := store.New(pool, store.WithChain())
	orgs := []string{chainOrg(t, pool, "race-a"), chainOrg(t, pool, "race-b"), chainOrg(t, pool, "race-c")}

	const writers = 50
	var wg sync.WaitGroup
	errs := make(chan error, writers*len(orgs))
	for _, org := range orgs {
		for n := 1; n <= writers; n++ {
			wg.Add(1)
			go func(org string, n int) {
				defer wg.Done()
				if _, err := st.InsertAudit(ctx, chainEvent(org, n)); err != nil {
					errs <- fmt.Errorf("org %s writer %d: %w", org, n, err)
				}
			}(org, n)
		}
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		t.Fatal(err)
	}
	for _, org := range orgs {
		rows := loadChain(t, ctx, pool, org)
		if len(rows) != writers {
			t.Fatalf("org %s chained rows = %d, want %d", org, len(rows), writers)
		}
		assertChainValid(t, org, rows)
		assertHeadIsTip(t, ctx, pool, org, rows)
	}
}

func TestChainedReplayDoesNotAdvanceHead(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "replay")
	st := store.New(pool, store.WithChain())
	bus := fmt.Sprintf("chain-replay-%d", time.Now().UnixNano())
	subject := "verevon.audit.v2.control.audit-core-test.chain_test"

	ev := chainEvent(org, 1)
	inserted, err := st.InsertAuditFromStream(ctx, ev, bus, subject, 42)
	if err != nil || !inserted {
		t.Fatalf("first delivery: inserted=%v err=%v", inserted, err)
	}
	for _, seq := range []uint64{42, 5043} { // redelivery, then a replay under a new stream sequence
		inserted, err = st.InsertAuditFromStream(ctx, ev, bus, subject, seq)
		if err != nil || inserted {
			t.Fatalf("replay at %d must be an acknowledged duplicate: inserted=%v err=%v", seq, inserted, err)
		}
	}
	rows := loadChain(t, ctx, pool, org)
	if len(rows) != 1 {
		t.Fatalf("rows after replays = %d, want 1", len(rows))
	}
	assertChainValid(t, org, rows)
	assertHeadIsTip(t, ctx, pool, org, rows)

	if _, err := st.InsertAuditFromStream(ctx, chainEvent(org, 2), bus, subject, 43); err != nil {
		t.Fatal(err)
	}
	rows = loadChain(t, ctx, pool, org)
	if len(rows) != 2 {
		t.Fatalf("rows after the next new event = %d, want 2 (a replay must not burn a sequence)", len(rows))
	}
	assertChainValid(t, org, rows)
}

func TestChainedChangedPayloadStillConflictsOnEveryInsertPath(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "conflict")
	st := store.New(pool, store.WithChain())
	bus := fmt.Sprintf("chain-conflict-%d", time.Now().UnixNano())

	changed := func(ev *events.AuditEvent) *events.AuditEvent {
		c := *ev
		c.Outcome = "denied"
		return &c
	}

	direct := chainEvent(org, 1)
	if _, err := st.InsertAudit(ctx, direct); err != nil {
		t.Fatal(err)
	}
	if _, err := st.InsertAudit(ctx, changed(direct)); !errors.Is(err, store.ErrAuditEventConflict) {
		t.Fatalf("InsertAudit with a changed payload: %v, want ErrAuditEventConflict", err)
	}

	source := chainEvent(org, 2)
	if inserted, err := st.InsertAuditFromSource(ctx, source, "http:audit-core-test"); err != nil || !inserted {
		t.Fatalf("InsertAuditFromSource: inserted=%v err=%v", inserted, err)
	}
	if _, err := st.InsertAuditFromSource(ctx, changed(source), "http:audit-core-test"); !errors.Is(err, store.ErrAuditEventConflict) {
		t.Fatalf("InsertAuditFromSource with a changed payload: %v, want ErrAuditEventConflict", err)
	}

	stream := chainEvent(org, 3)
	subject := "verevon.audit.v2.control.audit-core-test.chain_test"
	if inserted, err := st.InsertAuditFromStream(ctx, stream, bus, subject, 7); err != nil || !inserted {
		t.Fatalf("InsertAuditFromStream: inserted=%v err=%v", inserted, err)
	}
	if _, err := st.InsertAuditFromStream(ctx, changed(stream), bus, subject, 8); !errors.Is(err, store.ErrAuditEventConflict) {
		t.Fatalf("InsertAuditFromStream with a changed payload: %v, want ErrAuditEventConflict", err)
	}

	rows := loadChain(t, ctx, pool, org)
	if len(rows) != 3 {
		t.Fatalf("rows = %d, want 3 (a conflict must not append)", len(rows))
	}
	assertChainValid(t, org, rows)
	assertHeadIsTip(t, ctx, pool, org, rows)
}

func TestChainCatchUpChainsRowsInsertedWhileTheSwitchWasOff(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "catchup")
	on := store.New(pool, store.WithChain())
	off := store.New(pool)

	for n := 1; n <= 2; n++ {
		if _, err := on.InsertAudit(ctx, chainEvent(org, n)); err != nil {
			t.Fatal(err)
		}
	}
	for n := 3; n <= 5; n++ { // switch off: plain inserts, no chain position
		if _, err := off.InsertAudit(ctx, chainEvent(org, n)); err != nil {
			t.Fatal(err)
		}
	}
	var unchained int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM audit_events WHERE org_id = $1 AND chain_seq IS NULL`, org).Scan(&unchained); err != nil {
		t.Fatal(err)
	}
	if unchained != 3 {
		t.Fatalf("rows inserted with the switch off and chain_seq NULL = %d, want 3", unchained)
	}

	chained, err := on.CatchUp(ctx)
	if err != nil {
		t.Fatalf("CatchUp: %v", err)
	}
	if chained < 3 {
		t.Fatalf("CatchUp chained %d rows, want at least 3", chained)
	}
	rows := loadChain(t, ctx, pool, org)
	if len(rows) != 5 {
		t.Fatalf("chained rows after catch-up = %d, want 5", len(rows))
	}
	assertChainValid(t, org, rows)
	assertHeadIsTip(t, ctx, pool, org, rows)
	var modes []string
	for _, c := range rows {
		modes = append(modes, c.mode)
	}
	if got := strings.Join(modes, ","); got != "insert,insert,catchup,catchup,catchup" {
		t.Fatalf("chain modes = %s", got)
	}

	if again, err := on.CatchUp(ctx); err != nil || again != 0 {
		t.Fatalf("second CatchUp chained %d rows (err %v), want 0", again, err)
	}
}

func TestChainCatchUpLeavesLegacyRowsUnchained(t *testing.T) {
	ctx, pool := chainDB(t)
	never := chainOrg(t, pool, "never-chained")
	preexisting := chainOrg(t, pool, "preexisting")
	on := store.New(pool, store.WithChain())
	off := store.New(pool)

	// An org that never had the chain on has no head, so none of its rows may
	// be chained.
	if _, err := off.InsertAudit(ctx, chainEvent(never, 1)); err != nil {
		t.Fatal(err)
	}
	// A row written before the org's chain was activated stays legacy_unchained.
	if _, err := off.InsertAudit(ctx, chainEvent(preexisting, 1)); err != nil {
		t.Fatal(err)
	}
	time.Sleep(20 * time.Millisecond)
	if _, err := on.InsertAudit(ctx, chainEvent(preexisting, 2)); err != nil {
		t.Fatal(err)
	}

	if _, err := on.CatchUp(ctx); err != nil {
		t.Fatal(err)
	}
	if _, ok := loadHead(t, ctx, pool, never); ok {
		t.Fatal("catch-up created a head for an org that never had the chain on")
	}
	var neverChained, legacy int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM audit_events WHERE org_id = $1 AND chain_seq IS NOT NULL`, never).Scan(&neverChained); err != nil {
		t.Fatal(err)
	}
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM audit_events WHERE org_id = $1 AND chain_seq IS NULL`, preexisting).Scan(&legacy); err != nil {
		t.Fatal(err)
	}
	if neverChained != 0 || legacy != 1 {
		t.Fatalf("never-chained org has %d chained rows (want 0); pre-activation legacy rows = %d (want 1)", neverChained, legacy)
	}
	rows := loadChain(t, ctx, pool, preexisting)
	if len(rows) != 1 || rows[0].seq != 1 {
		t.Fatalf("pre-existing org chain = %d rows, want exactly the post-activation row at seq 1", len(rows))
	}
	assertChainValid(t, preexisting, rows)
}

func TestChainSwitchOffInsertsLeaveNoChainTrace(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "off")
	st := store.New(pool)
	if _, err := st.InsertAudit(ctx, chainEvent(org, 1)); err != nil {
		t.Fatal(err)
	}
	var seq, prev, hash, version, mode any
	if err := pool.QueryRow(ctx, `SELECT chain_seq, prev_hash, row_hash, chain_version, chain_mode FROM audit_events WHERE org_id = $1`, org).
		Scan(&seq, &prev, &hash, &version, &mode); err != nil {
		t.Fatal(err)
	}
	if seq != nil || prev != nil || hash != nil || version != nil || mode != nil {
		t.Fatalf("a switch-off insert wrote chain columns: %v %v %v %v %v", seq, prev, hash, version, mode)
	}
	if _, ok := loadHead(t, ctx, pool, org); ok {
		t.Fatal("a switch-off insert created a chain head")
	}
}

func TestAuditEventsGuardBlocksUpdateAndDeleteOutsideMaintenance(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "guard")
	on := store.New(pool, store.WithChain())
	off := store.New(pool)
	if _, err := on.InsertAudit(ctx, chainEvent(org, 1)); err != nil {
		t.Fatal(err)
	}
	if _, err := off.InsertAudit(ctx, chainEvent(org, 2)); err != nil {
		t.Fatal(err)
	}

	isGuard := func(err error) bool {
		var pgErr *pgconn.PgError
		return errors.As(err, &pgErr) && pgErr.Code == "23001" && strings.Contains(pgErr.Message, "append-only")
	}
	for name, stmt := range map[string]string{
		"edit a chained field":      `UPDATE audit_events SET outcome = 'denied' WHERE org_id = $1 AND chain_seq = 1`,
		"edit chained details":      `UPDATE audit_events SET details = '{"x":1}'::jsonb WHERE org_id = $1 AND chain_seq = 1`,
		"rewrite a chain position":  `UPDATE audit_events SET chain_seq = 9 WHERE org_id = $1 AND chain_seq = 1`,
		"rewrite a stored hash":     `UPDATE audit_events SET row_hash = prev_hash WHERE org_id = $1 AND chain_seq = 1`,
		"delete a chained row":      `DELETE FROM audit_events WHERE org_id = $1 AND chain_seq = 1`,
		"backdate an unchained row": `UPDATE audit_events SET ingested_at = NOW() - INTERVAL '400 days' WHERE org_id = $1 AND chain_seq IS NULL`,
		"delete an unchained row":   `DELETE FROM audit_events WHERE org_id = $1 AND chain_seq IS NULL`,
		"chain a row with a changed field": `UPDATE audit_events SET chain_seq = 2, prev_hash = '\x00', row_hash = '\x00',
			chain_version = 1, chain_mode = 'insert', outcome = 'denied' WHERE org_id = $1 AND chain_seq IS NULL`,
	} {
		if _, err := pool.Exec(ctx, stmt, org); !isGuard(err) {
			t.Errorf("%s: err = %v, want the append-only guard", name, err)
		}
	}

	var total int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM audit_events WHERE org_id = $1`, org).Scan(&total); err != nil {
		t.Fatal(err)
	}
	if total != 2 {
		t.Fatalf("rows after blocked statements = %d, want 2", total)
	}

	// Inside a maintenance transaction the same statement goes through.
	if err := execMaintenance(ctx, pool, `UPDATE audit_events SET ingested_at = NOW() - INTERVAL '400 days' WHERE org_id = $1 AND chain_seq IS NULL`, org); err != nil {
		t.Fatalf("maintenance update: %v", err)
	}
}

// Until the chain-safe purge exists, retention must not delete a chained row
// (that would break the org's chain) and must still work under the guard.
func TestPurgeSkipsChainedRowsAndStillRemovesExpiredUnchainedRows(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "purge")
	on := store.New(pool, store.WithChain())
	off := store.New(pool)
	if _, err := on.InsertAudit(ctx, chainEvent(org, 1)); err != nil {
		t.Fatal(err)
	}
	if _, err := off.InsertAudit(ctx, chainEvent(org, 2)); err != nil {
		t.Fatal(err)
	}
	if err := execMaintenance(ctx, pool, `UPDATE audit_events SET ingested_at = NOW() - INTERVAL '400 days' WHERE org_id = $1`, org); err != nil {
		t.Fatal(err)
	}

	if _, err := off.Purge(ctx, 365); err != nil {
		t.Fatalf("Purge: %v", err)
	}
	rows := loadChain(t, ctx, pool, org)
	if len(rows) != 1 {
		t.Fatalf("chained rows after purge = %d, want the expired chained row kept", len(rows))
	}
	assertChainValid(t, org, rows)
	var unchained int
	if err := pool.QueryRow(ctx, `SELECT COUNT(*) FROM audit_events WHERE org_id = $1 AND chain_seq IS NULL`, org).Scan(&unchained); err != nil {
		t.Fatal(err)
	}
	if unchained != 0 {
		t.Fatalf("expired unchained rows left after purge = %d, want 0", unchained)
	}
}

func TestChainedInsertMarksABrokenTipAndStillAppends(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "broken")
	st := store.New(pool, store.WithChain())
	for n := 1; n <= 2; n++ {
		if _, err := st.InsertAudit(ctx, chainEvent(org, n)); err != nil {
			t.Fatal(err)
		}
	}
	headBefore, _ := loadHead(t, ctx, pool, org)
	brokenBefore := counterValue(t, "audit_core_chain_broken_total", nil)

	if err := execMaintenance(ctx, pool, `UPDATE audit_events SET row_hash = decode(repeat('00', 32), 'hex') WHERE org_id = $1 AND chain_seq = 2`, org); err != nil {
		t.Fatal(err)
	}
	for n := 3; n <= 4; n++ {
		if _, err := st.InsertAudit(ctx, chainEvent(org, n)); err != nil {
			t.Fatalf("insert %d after tampering must still append: %v", n, err)
		}
	}
	head, _ := loadHead(t, ctx, pool, org)
	if head.state != "broken" || head.seq != 4 {
		t.Fatalf("head after tampering = seq %d state %s, want seq 4 state broken", head.seq, head.state)
	}
	if got := counterValue(t, "audit_core_chain_broken_total", nil) - brokenBefore; got != 1 {
		t.Fatalf("chain_broken_total grew by %v, want 1 (counted once per transition)", got)
	}
	var prev []byte
	if err := pool.QueryRow(ctx, `SELECT prev_hash FROM audit_events WHERE org_id = $1 AND chain_seq = 3`, org).Scan(&prev); err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(prev, headBefore.hash) {
		t.Fatal("the row appended after tampering must link to the head, not to the tampered row hash")
	}
}

func TestChainedInsertCountsDetailsGuardAndAppends(t *testing.T) {
	ctx, pool := chainDB(t)
	org := chainOrg(t, pool, "guardmetric")
	st := store.New(pool, store.WithChain())

	read := func(result string) float64 {
		return counterValue(t, "audit_core_details_guard_total", map[string]string{"result": result})
	}
	okBefore, deniedBefore, oversizeBefore := read("ok"), read("denied_key"), read("oversize")
	appendedBefore := counterValue(t, "audit_core_chain_appends_total", map[string]string{"result": "appended"})

	clean := chainEvent(org, 1)
	leaky := chainEvent(org, 2)
	leaky.Details = map[string]any{"nested": map[string]any{"Prompt": "summarise this contract"}}
	big := chainEvent(org, 3)
	big.Details = map[string]any{"blob": strings.Repeat("x", 9000)}
	for _, ev := range []*events.AuditEvent{clean, leaky, big} {
		if _, err := st.InsertAudit(ctx, ev); err != nil {
			t.Fatalf("observe-only guard must never reject: %v", err)
		}
	}
	if got := read("ok") - okBefore; got != 1 {
		t.Errorf("ok grew by %v, want 1", got)
	}
	if got := read("denied_key") - deniedBefore; got != 1 {
		t.Errorf("denied_key grew by %v, want 1", got)
	}
	if got := read("oversize") - oversizeBefore; got != 1 {
		t.Errorf("oversize grew by %v, want 1", got)
	}
	if got := counterValue(t, "audit_core_chain_appends_total", map[string]string{"result": "appended"}) - appendedBefore; got != 3 {
		t.Errorf("chain_appends_total{appended} grew by %v, want 3", got)
	}
}

// goldenFile is internal/testdata/export-content-free.golden.json: what the
// session-core wire fixtures look like once chained, so a change that lets
// content into the hashed or exported columns changes the golden.
type goldenFile struct {
	AllowedDetailsKeys []string `json:"allowed_details_keys"`
	Fixtures           map[string]struct {
		DetailsCanonical string `json:"details_canonical"`
		GenesisRowHash   string `json:"genesis_row_hash"`
	} `json:"fixtures"`
}

func loadGolden(t *testing.T) goldenFile {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("..", "testdata", "export-content-free.golden.json"))
	if err != nil {
		t.Fatal(err)
	}
	var g goldenFile
	if err := json.Unmarshal(raw, &g); err != nil {
		t.Fatal(err)
	}
	return g
}

func loadWireFixture(t *testing.T, name string) *events.AuditEvent {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join("..", "testdata", "session-core-wire", name))
	if err != nil {
		t.Fatal(err)
	}
	var parsed struct {
		Body json.RawMessage `json:"body"`
	}
	if err := json.Unmarshal(raw, &parsed); err != nil {
		t.Fatal(err)
	}
	ev, err := events.DecodeAudit(parsed.Body)
	if err != nil {
		t.Fatalf("audit-core refuses fixture %s: %v", name, err)
	}
	return ev
}

func sortedKeys(m map[string]any) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

// The hash of a chained session-core event is pinned with a fixed ingest time,
// sequence and bus, and its details may only carry the documented keys. Needs
// no database.
func TestSessionCoreFixturesHashContentFreeGolden(t *testing.T) {
	golden := loadGolden(t)
	allowed := map[string]bool{}
	for _, k := range golden.AllowedDetailsKeys {
		allowed[k] = true
	}
	ingestedAt := time.Date(2026, 10, 3, 12, 0, 1, 0, time.UTC)

	for name, want := range golden.Fixtures {
		ev := loadWireFixture(t, name)
		for _, key := range sortedKeys(ev.Details) {
			if !allowed[key] {
				t.Errorf("%s: details key %q is outside the content-free set %v", name, key, golden.AllowedDetailsKeys)
			}
		}
		details, err := json.Marshal(ev.Details)
		if err != nil {
			t.Fatal(err)
		}
		canonical, err := chain.CanonicalDetails(details)
		if err != nil {
			t.Fatal(err)
		}
		if string(canonical) != want.DetailsCanonical {
			t.Errorf("%s: canonical details\n got %s\nwant %s", name, canonical, want.DetailsCanonical)
		}
		payload, err := json.Marshal(ev)
		if err != nil {
			t.Fatal(err)
		}
		payloadHash := sha256.Sum256(payload)
		prev, err := chain.GenesisHash(ev.OrgID)
		if err != nil {
			t.Fatal(err)
		}
		hash, err := chain.RowHash(prev, chain.Row{
			OrgID: ev.OrgID, ChainSeq: 1, OccurredAt: ev.OccurredAt, IngestedAt: ingestedAt,
			UserID: ev.UserID, ActorRole: ev.ActorRole, Plane: ev.Plane, Event: ev.Event,
			Subject: ev.Subject, ResourceID: ev.ResourceID, Outcome: ev.Outcome, EventID: ev.EventID,
			RequestID: ev.RequestID, UserAgent: ev.UserAgent, SourceProducer: ev.Producer,
			SourceBus: "model", PayloadHash: payloadHash[:], Details: canonical,
		})
		if err != nil {
			t.Fatal(err)
		}
		if got := hex.EncodeToString(hash); got != want.GenesisRowHash {
			t.Errorf("%s: genesis row hash = %s, want %s", name, got, want.GenesisRowHash)
		}
	}
}

// Through the real insert path the stored details of the session-core fixtures
// are exactly the golden's canonical form and chain cleanly.
func TestChainedSessionCoreFixturesStoreOnlyContentFreeDetails(t *testing.T) {
	ctx, pool := chainDB(t)
	st := store.New(pool, store.WithChain())
	golden := loadGolden(t)
	bus := fmt.Sprintf("chain-golden-%d", time.Now().UnixNano())

	names := make([]string, 0, len(golden.Fixtures))
	for name := range golden.Fixtures {
		names = append(names, name)
	}
	sort.Strings(names)

	org := chainOrg(t, pool, "golden")
	for i, name := range names {
		ev := loadWireFixture(t, name)
		ev.OrgID = org
		subject := "verevon.audit.v2.model.session-core." + ev.Event
		if inserted, err := st.InsertAuditFromStream(ctx, ev, bus, subject, uint64(i+1)); err != nil || !inserted {
			t.Fatalf("%s: inserted=%v err=%v", name, inserted, err)
		}
	}
	rows := loadChain(t, ctx, pool, org)
	if len(rows) != len(names) {
		t.Fatalf("chained %d rows, want %d", len(rows), len(names))
	}
	assertChainValid(t, org, rows)
	for _, c := range rows {
		canonical, err := chain.CanonicalDetails(c.row.Details)
		if err != nil {
			t.Fatal(err)
		}
		matched := false
		for _, want := range golden.Fixtures {
			matched = matched || string(canonical) == want.DetailsCanonical
		}
		if !matched {
			t.Errorf("seq %d: stored details %s match no golden fixture", c.seq, canonical)
		}
	}
}
