package store_test

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// These tests need Postgres (TEST_DATABASE_URL) like the rest of the chain
// tests. Expired rows cannot be inserted directly (ingested_at is the insert
// time and part of the row hash), so backdateAndRechain ages rows the way a
// real, old chain would look: it moves ingested_at back and recomputes the
// hashes from there on, leaving a chain that verifies.

// pruneRing builds a signing key ring from a deterministic seed.
func pruneRing(t *testing.T, keyID string, seedByte byte, verifyKeys string) *chain.KeyRing {
	t.Helper()
	ring, err := chain.NewKeyRing(chain.KeyConfig{
		KeyID:      keyID,
		Private:    base64.RawURLEncoding.EncodeToString(seedOf(seedByte)),
		VerifyKeys: verifyKeys,
	})
	if err != nil {
		t.Fatal(err)
	}
	return ring
}

func seedOf(b byte) []byte {
	seed := make([]byte, ed25519.SeedSize)
	for i := range seed {
		seed[i] = b + byte(i)
	}
	return seed
}

func publicOf(b byte) string {
	pub := ed25519.NewKeyFromSeed(seedOf(b)).Public().(ed25519.PublicKey)
	return base64.RawURLEncoding.EncodeToString(pub)
}

// pruneOrg is chainOrg plus removal of the S3 tables' rows for the org.
func pruneOrg(t *testing.T, pool *pgxpool.Pool, label string) string {
	t.Helper()
	org := chainOrg(t, pool, label)
	t.Cleanup(func() {
		ctx := context.Background()
		for _, table := range []string{"audit_chain_checkpoints", "audit_chain_attestations", "audit_chain_incidents"} {
			_, _ = pool.Exec(ctx, `DELETE FROM `+table+` WHERE org_id = $1`, org)
		}
	})
	return org
}

func insertChained(t *testing.T, ctx context.Context, st *store.Store, org string, from, to int) {
	t.Helper()
	for n := from; n <= to; n++ {
		if _, err := st.InsertAudit(ctx, chainEvent(org, n)); err != nil {
			t.Fatalf("insert %d: %v", n, err)
		}
	}
}

// backdateAndRechain makes rows 1..expiredThrough (by chain_seq) 400 days old
// and recomputes every stored hash from the first present row onwards, so the
// chain still verifies. The first row keeps its stored prev_hash, which is the
// previous checkpoint's hash when the chain has already been pruned once.
func backdateAndRechain(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string, expiredThrough int64) {
	t.Helper()
	rows := loadChain(t, ctx, pool, org)
	if len(rows) == 0 {
		t.Fatal("no chained rows to age")
	}
	now := time.Now().UTC().Truncate(time.Microsecond)
	prev := rows[0].prev
	for i := range rows {
		r := &rows[i]
		if r.seq <= expiredThrough {
			r.row.IngestedAt = now.Add(-400*24*time.Hour + time.Duration(r.seq)*time.Millisecond)
		}
		hash, err := chain.RowHash(prev, r.row)
		if err != nil {
			t.Fatal(err)
		}
		if err := execMaintenance(ctx, pool, `
			UPDATE audit_events SET ingested_at = $2, prev_hash = $3, row_hash = $4 WHERE id = $1`,
			r.id, r.row.IngestedAt, prev, hash); err != nil {
			t.Fatal(err)
		}
		prev = hash
	}
	if _, err := pool.Exec(ctx, `UPDATE audit_chain_heads SET head_hash = $2 WHERE org_id = $1`, org, prev); err != nil {
		t.Fatal(err)
	}
}

type checkpointRow struct {
	throughSeq int64
	hash       []byte
	total      int64
	reason     string
	keyID      string
	signature  []byte
	createdAt  time.Time
}

func loadCheckpoints(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string) []checkpointRow {
	t.Helper()
	rows, err := pool.Query(ctx, `
		SELECT through_seq, through_hash, pruned_rows_total, reason, key_id, signature, created_at
		FROM audit_chain_checkpoints WHERE org_id = $1 ORDER BY through_seq`, org)
	if err != nil {
		t.Fatal(err)
	}
	defer rows.Close()
	var out []checkpointRow
	for rows.Next() {
		var c checkpointRow
		if err := rows.Scan(&c.throughSeq, &c.hash, &c.total, &c.reason, &c.keyID, &c.signature, &c.createdAt); err != nil {
			t.Fatal(err)
		}
		out = append(out, c)
	}
	if err := rows.Err(); err != nil {
		t.Fatal(err)
	}
	return out
}

func assertCheckpointsVerify(t *testing.T, ring *chain.KeyRing, org string, cps []checkpointRow) {
	t.Helper()
	var lastSeq, lastTotal int64
	for _, c := range cps {
		if err := ring.VerifyCheckpoint(org, c.throughSeq, c.hash, c.total, c.createdAt, c.keyID, c.signature); err != nil {
			t.Fatalf("checkpoint through %d does not verify: %v", c.throughSeq, err)
		}
		if c.throughSeq <= lastSeq || c.total <= lastTotal {
			t.Fatalf("checkpoint through %d (total %d) does not advance past %d (total %d)", c.throughSeq, c.total, lastSeq, lastTotal)
		}
		if c.reason != "retention" {
			t.Fatalf("checkpoint reason = %q, want retention", c.reason)
		}
		lastSeq, lastTotal = c.throughSeq, c.total
	}
}

// assertSuffixValid is assertChainValid for a chain that starts after a
// checkpoint: rows run contiguously from firstSeq and the first links to prev.
func assertSuffixValid(t *testing.T, rows []chainedRow, firstSeq int64, prev []byte) {
	t.Helper()
	for i, c := range rows {
		if c.seq != firstSeq+int64(i) {
			t.Fatalf("row %d has chain_seq %d, want %d", i, c.seq, firstSeq+int64(i))
		}
		if !bytes.Equal(c.prev, prev) {
			t.Fatalf("seq %d: prev_hash does not link to the previous row or checkpoint", c.seq)
		}
		want, err := chain.RowHash(prev, c.row)
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(c.hash, want) {
			t.Fatalf("seq %d: stored row_hash differs from a recomputation", c.seq)
		}
		prev = c.hash
	}
}

type incidentRow struct {
	kind   string
	badSeq *int64
}

func loadIncidents(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string) []incidentRow {
	t.Helper()
	rows, err := pool.Query(ctx,
		`SELECT kind, first_bad_seq FROM audit_chain_incidents WHERE org_id = $1 ORDER BY id`, org)
	if err != nil {
		t.Fatal(err)
	}
	defer rows.Close()
	var out []incidentRow
	for rows.Next() {
		var i incidentRow
		if err := rows.Scan(&i.kind, &i.badSeq); err != nil {
			t.Fatal(err)
		}
		out = append(out, i)
	}
	if err := rows.Err(); err != nil {
		t.Fatal(err)
	}
	return out
}

func countChained(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string) int {
	t.Helper()
	var n int
	if err := pool.QueryRow(ctx,
		`SELECT COUNT(*) FROM audit_events WHERE org_id = $1 AND chain_seq IS NOT NULL`, org).Scan(&n); err != nil {
		t.Fatal(err)
	}
	return n
}

func TestPurgeWritesSignedCheckpointAndChainStillVerifies(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "ckpt")
	ring := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneOn))

	insertChained(t, ctx, st, org, 1, 20)
	backdateAndRechain(t, ctx, pool, org, 10)
	before := loadChain(t, ctx, pool, org)
	tenth := before[9].hash

	res, err := st.Purge(ctx, 365)
	if err != nil {
		t.Fatalf("Purge: %v", err)
	}
	if res.CheckpointsWritten < 1 {
		t.Fatalf("CheckpointsWritten = %d, want at least 1", res.CheckpointsWritten)
	}

	rows := loadChain(t, ctx, pool, org)
	if len(rows) != 10 {
		t.Fatalf("chained rows after purge = %d, want the 10 unexpired ones", len(rows))
	}
	cps := loadCheckpoints(t, ctx, pool, org)
	if len(cps) != 1 {
		t.Fatalf("checkpoints = %d, want 1", len(cps))
	}
	cp := cps[0]
	if cp.throughSeq != 10 || cp.total != 10 || !bytes.Equal(cp.hash, tenth) {
		t.Fatalf("checkpoint = through %d total %d, want through 10 total 10 at the hash of row 10", cp.throughSeq, cp.total)
	}
	assertCheckpointsVerify(t, ring, org, cps)
	assertSuffixValid(t, rows, 11, cp.hash)
	assertHeadIsTip(t, ctx, pool, org, rows)
	if inc := loadIncidents(t, ctx, pool, org); len(inc) != 0 {
		t.Fatalf("a clean purge wrote incidents: %v", inc)
	}

	// The chain keeps growing from the head, and a second sweep with nothing
	// expired writes nothing.
	insertChained(t, ctx, st, org, 21, 22)
	if _, err := st.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	rows = loadChain(t, ctx, pool, org)
	if len(rows) != 12 {
		t.Fatalf("chained rows = %d, want 12", len(rows))
	}
	assertSuffixValid(t, rows, 11, cp.hash)
	if got := len(loadCheckpoints(t, ctx, pool, org)); got != 1 {
		t.Fatalf("a purge with nothing expired wrote a checkpoint: %d checkpoints", got)
	}
	if head, _ := loadHead(t, ctx, pool, org); head.state != "ok" {
		t.Fatalf("head state = %s after purging, want ok", head.state)
	}
}

func TestPurgeBatchesWriteOneCheckpointEach(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "batches")
	ring := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneOn),
		store.WithPruneBatchSizeForTest(4))

	insertChained(t, ctx, st, org, 1, 20)
	backdateAndRechain(t, ctx, pool, org, 10)
	if _, err := st.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}

	cps := loadCheckpoints(t, ctx, pool, org)
	if len(cps) != 3 {
		t.Fatalf("checkpoints = %d, want 3 (batches of 4, 4 and 2)", len(cps))
	}
	for i, want := range []struct{ through, total int64 }{{4, 4}, {8, 8}, {10, 10}} {
		if cps[i].throughSeq != want.through || cps[i].total != want.total {
			t.Fatalf("checkpoint %d = through %d total %d, want through %d total %d",
				i, cps[i].throughSeq, cps[i].total, want.through, want.total)
		}
	}
	assertCheckpointsVerify(t, ring, org, cps)
	rows := loadChain(t, ctx, pool, org)
	assertSuffixValid(t, rows, 11, cps[2].hash)
}

func TestPurgeRefusesATamperedPrefixAndDeletesNothing(t *testing.T) {
	for name, tc := range map[string]struct {
		tamper string
		kind   string
		badSeq int64
	}{
		"edited details": {
			tamper: `UPDATE audit_events SET details = '{"n": 999}'::jsonb WHERE org_id = $1 AND chain_seq = 5`,
			kind:   "row_hash_mismatch", badSeq: 5,
		},
		"deleted middle row": {
			tamper: `DELETE FROM audit_events WHERE org_id = $1 AND chain_seq = 4`,
			kind:   "sequence_gap", badSeq: 4,
		},
		"forged prev_hash": {
			tamper: `UPDATE audit_events SET prev_hash = decode(repeat('ab', 32), 'hex') WHERE org_id = $1 AND chain_seq = 3`,
			kind:   "prev_hash_mismatch", badSeq: 3,
		},
		"edited stored row_hash": {
			tamper: `UPDATE audit_events SET row_hash = decode(repeat('cd', 32), 'hex') WHERE org_id = $1 AND chain_seq = 7`,
			kind:   "row_hash_mismatch", badSeq: 7,
		},
		"deleted tail of the prefix": {
			// Rows 1..9 remain, the purge wants 1..10: the missing tail is a gap.
			tamper: `DELETE FROM audit_events WHERE org_id = $1 AND chain_seq = 10`,
			kind:   "sequence_gap", badSeq: 10,
		},
		"head already marked broken": {
			tamper: `UPDATE audit_chain_heads SET state = 'broken' WHERE org_id = $1`,
			kind:   "chain_marked_broken", badSeq: 0,
		},
	} {
		tc := tc
		t.Run(name, func(t *testing.T) {
			ctx, pool := chainDB(t)
			org := pruneOrg(t, pool, "tamper")
			ring := pruneRing(t, "audit-test-a", 1, "")
			st := store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneOn))
			insertChained(t, ctx, st, org, 1, 20)
			backdateAndRechain(t, ctx, pool, org, 10)
			if err := execMaintenance(ctx, pool, tc.tamper, org); err != nil {
				t.Fatal(err)
			}
			rowsBefore := countChained(t, ctx, pool, org)

			res, err := st.Purge(ctx, 365)
			if err != nil {
				t.Fatalf("Purge must report a blocked org, not fail: %v", err)
			}
			if res.OrgsBlocked < 1 {
				t.Fatalf("OrgsBlocked = %d, want at least 1", res.OrgsBlocked)
			}
			if got := countChained(t, ctx, pool, org); got != rowsBefore {
				t.Fatalf("chained rows %d -> %d: a purge of a prefix that does not verify must delete nothing", rowsBefore, got)
			}
			if cps := loadCheckpoints(t, ctx, pool, org); len(cps) != 0 {
				t.Fatalf("a blocked purge wrote %d checkpoints", len(cps))
			}
			inc := loadIncidents(t, ctx, pool, org)
			if len(inc) != 1 || inc[0].kind != tc.kind {
				t.Fatalf("incidents = %+v, want one of kind %s", inc, tc.kind)
			}
			if tc.badSeq == 0 {
				if inc[0].badSeq != nil {
					t.Fatalf("first_bad_seq = %d, want NULL", *inc[0].badSeq)
				}
			} else if inc[0].badSeq == nil || *inc[0].badSeq != tc.badSeq {
				t.Fatalf("first_bad_seq = %v, want %d", inc[0].badSeq, tc.badSeq)
			}
		})
	}
}

func TestPurgeDuringConcurrentInserts(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "concurrent")
	ring := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneOn),
		store.WithPruneBatchSizeForTest(3))

	insertChained(t, ctx, st, org, 1, 10)
	backdateAndRechain(t, ctx, pool, org, 10)

	var wg sync.WaitGroup
	errs := make(chan error, 2)
	wg.Add(2)
	go func() {
		defer wg.Done()
		for n := 11; n <= 30; n++ {
			if _, err := st.InsertAudit(ctx, chainEvent(org, n)); err != nil {
				errs <- err
				return
			}
		}
	}()
	go func() {
		defer wg.Done()
		_, err := st.Purge(ctx, 365)
		if err != nil {
			errs <- err
		}
	}()
	wg.Wait()
	close(errs)
	for err := range errs {
		t.Fatal(err)
	}

	cps := loadCheckpoints(t, ctx, pool, org)
	if len(cps) == 0 {
		t.Fatal("the expired prefix was not purged")
	}
	assertCheckpointsVerify(t, ring, org, cps)
	rows := loadChain(t, ctx, pool, org)
	last := cps[len(cps)-1]
	if last.throughSeq != 10 {
		t.Fatalf("last checkpoint through %d, want 10: only the expired rows may be purged", last.throughSeq)
	}
	if len(rows) != 20 {
		t.Fatalf("chained rows = %d, want the 20 inserted during the purge", len(rows))
	}
	assertSuffixValid(t, rows, 11, last.hash)
	assertHeadIsTip(t, ctx, pool, org, rows)
	if head, _ := loadHead(t, ctx, pool, org); head.state != "ok" {
		t.Fatalf("head state = %s, want ok", head.state)
	}
}

func TestTwoRacingPurgersCutTheSamePrefixOnce(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "racing")
	ring := pruneRing(t, "audit-test-a", 1, "")
	newStore := func() *store.Store {
		return store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneOn),
			store.WithPruneBatchSizeForTest(3))
	}
	st := newStore()
	insertChained(t, ctx, st, org, 1, 20)
	backdateAndRechain(t, ctx, pool, org, 10)

	var wg sync.WaitGroup
	errs := make(chan error, 2)
	for i := 0; i < 2; i++ {
		wg.Add(1)
		go func(s *store.Store) {
			defer wg.Done()
			if _, err := s.Purge(ctx, 365); err != nil {
				errs <- err
			}
		}(newStore())
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		t.Fatal(err)
	}
	// A purger that found the org locked skipped it; the next sweep finishes.
	if _, err := st.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}

	rows := loadChain(t, ctx, pool, org)
	if len(rows) != 10 {
		t.Fatalf("chained rows = %d, want 10", len(rows))
	}
	cps := loadCheckpoints(t, ctx, pool, org)
	assertCheckpointsVerify(t, ring, org, cps)
	last := cps[len(cps)-1]
	if last.throughSeq != 10 || last.total != 10 {
		t.Fatalf("last checkpoint through %d total %d, want 10 and 10 (each row pruned once)", last.throughSeq, last.total)
	}
	assertSuffixValid(t, rows, 11, last.hash)
	if inc := loadIncidents(t, ctx, pool, org); len(inc) != 0 {
		t.Fatalf("racing purgers produced incidents: %v", inc)
	}
}

func TestPurgeOfAnEntirelyExpiredChainLeavesTheHeadUsable(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "allexpired")
	ring := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneOn))
	insertChained(t, ctx, st, org, 1, 6)
	backdateAndRechain(t, ctx, pool, org, 6)
	headBefore, _ := loadHead(t, ctx, pool, org)

	if _, err := st.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	if n := countChained(t, ctx, pool, org); n != 0 {
		t.Fatalf("chained rows left = %d, want 0", n)
	}
	cps := loadCheckpoints(t, ctx, pool, org)
	if len(cps) != 1 || cps[0].throughSeq != 6 || !bytes.Equal(cps[0].hash, headBefore.hash) {
		t.Fatalf("checkpoints = %+v, want one through 6 at the head hash", cps)
	}
	assertCheckpointsVerify(t, ring, org, cps)
	head, _ := loadHead(t, ctx, pool, org)
	if head.seq != 6 || !bytes.Equal(head.hash, headBefore.hash) || head.state != "ok" {
		t.Fatalf("head = seq %d state %s, want it untouched at seq 6", head.seq, head.state)
	}

	// The next append continues from the head even though no row carries it.
	insertChained(t, ctx, st, org, 7, 8)
	rows := loadChain(t, ctx, pool, org)
	assertSuffixValid(t, rows, 7, cps[0].hash)
	assertHeadIsTip(t, ctx, pool, org, rows)
	if head, _ := loadHead(t, ctx, pool, org); head.state != "ok" {
		t.Fatalf("head state = %s after appending past a pruned tip, want ok", head.state)
	}
}

func TestPruneOffKeepsExpiredChainedRowsAndPruneOnNeedsAKey(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "off")
	ring := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring))
	insertChained(t, ctx, st, org, 1, 10)
	backdateAndRechain(t, ctx, pool, org, 10)

	if _, err := st.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	if n := countChained(t, ctx, pool, org); n != 10 {
		t.Fatalf("with pruning off a purge removed chained rows: %d left, want 10", n)
	}
	if cps := loadCheckpoints(t, ctx, pool, org); len(cps) != 0 {
		t.Fatalf("pruning off wrote %d checkpoints", len(cps))
	}

	unsigned := store.New(pool, store.WithChain(), store.WithChainPrune(store.PruneOn))
	if _, err := unsigned.Purge(ctx, 365); !errors.Is(err, chain.ErrNoSigningKey) {
		t.Fatalf("pruning on without a signing key: %v, want ErrNoSigningKey", err)
	}
	if n := countChained(t, ctx, pool, org); n != 10 {
		t.Fatalf("a keyless purge removed chained rows: %d left, want 10", n)
	}
}

func TestPurgeDryRunVerifiesAndPlansButChangesNothing(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "dryrun")
	ring := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneDryRun),
		store.WithPruneBatchSizeForTest(4))
	insertChained(t, ctx, st, org, 1, 20)
	backdateAndRechain(t, ctx, pool, org, 10)

	res, err := st.Purge(ctx, 365)
	if err != nil {
		t.Fatal(err)
	}
	if res.ChainedPlanned < 10 {
		t.Fatalf("ChainedPlanned = %d, want at least the 10 expired rows of this org", res.ChainedPlanned)
	}
	if res.CheckpointsWritten != 0 {
		t.Fatalf("a dry run wrote %d checkpoints", res.CheckpointsWritten)
	}
	if n := countChained(t, ctx, pool, org); n != 20 {
		t.Fatalf("a dry run deleted rows: %d left, want 20", n)
	}
	if cps := loadCheckpoints(t, ctx, pool, org); len(cps) != 0 {
		t.Fatalf("a dry run wrote %d checkpoints", len(cps))
	}
	rows := loadChain(t, ctx, pool, org)
	assertChainValid(t, org, rows)

	// A dry run that finds a bad prefix still records the incident: the
	// operator learns about it before switching pruning on.
	if err := execMaintenance(ctx, pool,
		`UPDATE audit_events SET details = '{"n": 999}'::jsonb WHERE org_id = $1 AND chain_seq = 2`, org); err != nil {
		t.Fatal(err)
	}
	if _, err := st.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	if inc := loadIncidents(t, ctx, pool, org); len(inc) != 1 || inc[0].kind != "row_hash_mismatch" {
		t.Fatalf("dry-run incidents = %+v, want one row_hash_mismatch", inc)
	}
}

func TestPurgeBlocksOnACheckpointItCannotTrustAndRotationUnblocksIt(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "rotation")
	oldRing := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(oldRing), store.WithChainPrune(store.PruneOn))
	insertChained(t, ctx, st, org, 1, 10)
	backdateAndRechain(t, ctx, pool, org, 5)
	if _, err := st.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	if cps := loadCheckpoints(t, ctx, pool, org); len(cps) != 1 || cps[0].throughSeq != 5 {
		t.Fatalf("setup: checkpoints = %+v, want one through 5", cps)
	}
	backdateAndRechain(t, ctx, pool, org, 8)

	// A ring that dropped the old key cannot vouch for its checkpoint.
	forgotten := store.New(pool, store.WithChain(), store.WithChainKeys(pruneRing(t, "audit-test-b", 50, "")),
		store.WithChainPrune(store.PruneOn))
	if _, err := forgotten.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	if inc := loadIncidents(t, ctx, pool, org); len(inc) != 1 || inc[0].kind != "checkpoint_key_unknown" {
		t.Fatalf("incidents = %+v, want one checkpoint_key_unknown", inc)
	}
	if n := countChained(t, ctx, pool, org); n != 5 {
		t.Fatalf("chained rows = %d, want 5 (nothing deleted while the checkpoint is unverifiable)", n)
	}

	// With the retired key kept as a verify key the purge proceeds and signs
	// with the new key.
	newRing := pruneRing(t, "audit-test-b", 50, "audit-test-a:"+publicOf(1))
	rotated := store.New(pool, store.WithChain(), store.WithChainKeys(newRing), store.WithChainPrune(store.PruneOn))
	if _, err := rotated.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	cps := loadCheckpoints(t, ctx, pool, org)
	if len(cps) != 2 || cps[1].throughSeq != 8 || cps[1].keyID != "audit-test-b" || cps[0].keyID != "audit-test-a" {
		t.Fatalf("checkpoints after rotation = %+v, want through 5 (key a) then through 8 (key b)", cps)
	}
	assertCheckpointsVerify(t, newRing, org, cps)

	// A checkpoint whose signature was altered blocks the purge.
	if _, err := pool.Exec(ctx,
		`UPDATE audit_chain_checkpoints SET signature = decode(repeat('00', 64), 'hex') WHERE org_id = $1 AND through_seq = 8`, org); err != nil {
		t.Fatal(err)
	}
	backdateAndRechain(t, ctx, pool, org, 10)
	if _, err := rotated.Purge(ctx, 365); err != nil {
		t.Fatal(err)
	}
	inc := loadIncidents(t, ctx, pool, org)
	if len(inc) != 2 || inc[1].kind != "checkpoint_signature_invalid" {
		t.Fatalf("incidents = %+v, want a second one of kind checkpoint_signature_invalid", inc)
	}
	if n := countChained(t, ctx, pool, org); n != 2 {
		t.Fatalf("chained rows = %d, want 2", n)
	}
}

type attestationRow struct {
	seq  int64
	hash []byte
	key  string
	sig  []byte
	at   time.Time
}

func TestAttestSignsEachHeadOnceAndSkipsBrokenOnes(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "attest")
	broken := pruneOrg(t, pool, "attest-broken")
	ring := pruneRing(t, "audit-test-a", 1, "")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring))

	attestations := func(o string) []attestationRow {
		rows, err := pool.Query(ctx, `
			SELECT chain_seq, row_hash, key_id, signature, signed_at
			FROM audit_chain_attestations WHERE org_id = $1 ORDER BY chain_seq`, o)
		if err != nil {
			t.Fatal(err)
		}
		defer rows.Close()
		var out []attestationRow
		for rows.Next() {
			var a attestationRow
			if err := rows.Scan(&a.seq, &a.hash, &a.key, &a.sig, &a.at); err != nil {
				t.Fatal(err)
			}
			out = append(out, a)
		}
		return out
	}

	insertChained(t, ctx, st, org, 1, 3)
	insertChained(t, ctx, st, broken, 1, 2)
	if _, err := pool.Exec(ctx, `UPDATE audit_chain_heads SET state = 'broken' WHERE org_id = $1`, broken); err != nil {
		t.Fatal(err)
	}

	if _, err := st.Attest(ctx); err != nil {
		t.Fatal(err)
	}
	got := attestations(org)
	if len(got) != 1 || got[0].seq != 3 {
		t.Fatalf("attestations = %+v, want one at seq 3", got)
	}
	head, _ := loadHead(t, ctx, pool, org)
	if !bytes.Equal(got[0].hash, head.hash) {
		t.Fatal("the attestation does not cover the head hash")
	}
	if err := ring.VerifyAttestation(org, got[0].seq, got[0].hash, got[0].at, got[0].key, got[0].sig); err != nil {
		t.Fatalf("attestation does not verify: %v", err)
	}
	if n := len(attestations(broken)); n != 0 {
		t.Fatalf("a broken head was attested (%d attestations)", n)
	}

	// Nothing changed: nothing is signed again. One more row: one more signature.
	if _, err := st.Attest(ctx); err != nil {
		t.Fatal(err)
	}
	if n := len(attestations(org)); n != 1 {
		t.Fatalf("attestations after an idle tick = %d, want 1", n)
	}
	insertChained(t, ctx, st, org, 4, 4)
	if _, err := st.Attest(ctx); err != nil {
		t.Fatal(err)
	}
	if got := attestations(org); len(got) != 2 || got[1].seq != 4 {
		t.Fatalf("attestations = %+v, want a second at seq 4", got)
	}

	unsigned := store.New(pool, store.WithChain())
	if _, err := unsigned.Attest(ctx); !errors.Is(err, chain.ErrNoSigningKey) {
		t.Fatalf("Attest without a key: %v, want ErrNoSigningKey", err)
	}
}
