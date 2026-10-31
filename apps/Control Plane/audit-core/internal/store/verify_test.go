package store_test

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/store"
	"github.com/triodelab/controlplane/audit-core/internal/verifier"
)

// These tests need Postgres (TEST_DATABASE_URL) like the other chain tests.
// The verifier's decision rules are covered without a database in
// internal/verifier; what is proven here is that the SQL feeds those rules the
// right rows, anchors and attestations, so each attack of the plan's table is
// applied to real rows and must come back with the right first_broken_seq.

// ensureVerificationState applies migration 009 directly: it is registered in
// the migrations list by the lead (migrate.go is not part of this slice), and
// the DDL is idempotent, so this is harmless once it is.
func ensureVerificationState(t *testing.T, ctx context.Context, pool *pgxpool.Pool) {
	t.Helper()
	sql, err := os.ReadFile("migrations/009_audit_chain_verification_state.sql")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := pool.Exec(ctx, string(sql)); err != nil {
		t.Fatalf("apply verification state migration: %v", err)
	}
}

// verifyOrg is pruneOrg plus removal of the verifier's state row.
func verifyOrg(t *testing.T, pool *pgxpool.Pool, label string) string {
	t.Helper()
	org := pruneOrg(t, pool, label)
	t.Cleanup(func() {
		_, _ = pool.Exec(context.Background(), `DELETE FROM audit_chain_verifications WHERE org_id = $1`, org)
	})
	return org
}

type verifyEnv struct {
	ctx  context.Context
	pool *pgxpool.Pool
	st   *store.Store
	org  string
	ring *chain.KeyRing
}

func newVerifyEnv(t *testing.T, label string) verifyEnv {
	t.Helper()
	ctx, pool := chainDB(t)
	ensureVerificationState(t, ctx, pool)
	ring := pruneRing(t, "audit-test-a", 1, "")
	return verifyEnv{
		ctx: ctx, pool: pool, org: verifyOrg(t, pool, label), ring: ring,
		st: store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithChainPrune(store.PruneOn)),
	}
}

func (e verifyEnv) attest(t *testing.T) {
	t.Helper()
	if _, err := e.st.Attest(e.ctx); err != nil {
		t.Fatalf("Attest: %v", err)
	}
}

// newAttestedChain inserts rows 1..10 and attests the head at 5 and at 10.
func newAttestedChain(t *testing.T, label string) verifyEnv {
	t.Helper()
	e := newVerifyEnv(t, label)
	insertChained(t, e.ctx, e.st, e.org, 1, 5)
	e.attest(t)
	insertChained(t, e.ctx, e.st, e.org, 6, 10)
	e.attest(t)
	return e
}

func (e verifyEnv) verify(t *testing.T, opts verifier.Options) verifier.Report {
	t.Helper()
	rep, err := e.st.VerifyOrg(e.ctx, e.org, opts)
	if err != nil {
		t.Fatalf("VerifyOrg: %v", err)
	}
	return rep
}

func (e verifyEnv) maintenance(t *testing.T, sql string, args ...any) {
	t.Helper()
	if err := execMaintenance(e.ctx, e.pool, sql, args...); err != nil {
		t.Fatalf("%s: %v", sql, err)
	}
}

func (e verifyEnv) exec(t *testing.T, sql string, args ...any) {
	t.Helper()
	if _, err := e.pool.Exec(e.ctx, sql, args...); err != nil {
		t.Fatalf("%s: %v", sql, err)
	}
}

// rechainFrom is what an attacker without the signing key does: change the
// outcome of one row and recompute every hash after it (and the head), leaving
// a chain that is internally consistent.
func (e verifyEnv) rechainFrom(t *testing.T, seq int64, outcome string) {
	t.Helper()
	rows := loadChain(t, e.ctx, e.pool, e.org)
	var start = -1
	for i, r := range rows {
		if r.seq == seq {
			start = i
		}
	}
	if start < 0 {
		t.Fatalf("no chained row %d", seq)
	}
	prev := rows[start].prev
	for i := start; i < len(rows); i++ {
		r := &rows[i]
		if i == start {
			r.row.Outcome = outcome
		}
		hash, err := chain.RowHash(prev, r.row)
		if err != nil {
			t.Fatal(err)
		}
		e.maintenance(t, `UPDATE audit_events SET outcome = $2, prev_hash = $3, row_hash = $4 WHERE id = $1`,
			r.id, r.row.Outcome, prev, hash)
		prev = hash
	}
	e.exec(t, `UPDATE audit_chain_heads SET head_hash = $2 WHERE org_id = $1`, e.org, prev)
}

func assertReport(t *testing.T, rep verifier.Report, status verifier.Status, reason string, firstBroken int64) {
	t.Helper()
	if rep.Status != status || rep.Reason != reason || rep.FirstBrokenSeq != firstBroken {
		t.Fatalf("report = %s/%s first_broken_seq=%d, want %s/%s first_broken_seq=%d",
			rep.Status, rep.Reason, rep.FirstBrokenSeq, status, reason, firstBroken)
	}
}

func TestVerifyOrgCleanChainIsVerified(t *testing.T) {
	e := newAttestedChain(t, "clean")
	rep := e.verify(t, verifier.Options{})
	assertReport(t, rep, verifier.StatusVerified, "", 0)
	if rep.RowsVerified != 10 || rep.ThroughSeq != 10 || !rep.Complete || rep.LegacyUnchainedRows != 0 {
		t.Fatalf("coverage = %+v", rep)
	}
	if rep.Attested == nil || !rep.Attested.Verified || rep.Attested.Seq != 10 || rep.Head == nil || rep.Head.Seq != 10 {
		t.Fatalf("anchors = attested %+v head %+v", rep.Attested, rep.Head)
	}
}

func TestVerifyOrgAttackTable(t *testing.T) {
	cases := []struct {
		name   string
		attack func(t *testing.T, e verifyEnv)
		status verifier.Status
		reason string
		seq    int64
	}{
		{
			name: "edit a field",
			attack: func(t *testing.T, e verifyEnv) {
				e.maintenance(t, `UPDATE audit_events SET outcome = 'denied' WHERE org_id = $1 AND chain_seq = 4`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonRowHashMismatch, seq: 4,
		},
		{
			name: "edit details",
			attack: func(t *testing.T, e verifyEnv) {
				e.maintenance(t, `UPDATE audit_events SET details = '{"n": 99}'::jsonb WHERE org_id = $1 AND chain_seq = 6`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonRowHashMismatch, seq: 6,
		},
		{
			name: "delete a middle row",
			attack: func(t *testing.T, e verifyEnv) {
				e.maintenance(t, `DELETE FROM audit_events WHERE org_id = $1 AND chain_seq = 5`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonSequenceGap, seq: 5,
		},
		{
			name: "delete the tail",
			attack: func(t *testing.T, e verifyEnv) {
				e.maintenance(t, `DELETE FROM audit_events WHERE org_id = $1 AND chain_seq >= 9`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonTailMissing, seq: 9,
		},
		{
			name: "delete the tail and lower the head",
			attack: func(t *testing.T, e verifyEnv) {
				e.maintenance(t, `DELETE FROM audit_events WHERE org_id = $1 AND chain_seq >= 6`, e.org)
				e.exec(t, `UPDATE audit_chain_heads SET head_seq = 5,
					head_hash = (SELECT row_hash FROM audit_events WHERE org_id = $1 AND chain_seq = 5)
					WHERE org_id = $1`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonTruncated, seq: 6,
		},
		{
			name: "swap two chain_seq values",
			attack: func(t *testing.T, e verifyEnv) {
				// Simple protocol (no arguments) so the three statements run as one.
				e.maintenance(t, fmt.Sprintf(`
					UPDATE audit_events SET chain_seq = 1000 WHERE org_id = '%[1]s' AND chain_seq = 3;
					UPDATE audit_events SET chain_seq = 3 WHERE org_id = '%[1]s' AND chain_seq = 4;
					UPDATE audit_events SET chain_seq = 4 WHERE org_id = '%[1]s' AND chain_seq = 1000`, e.org))
			},
			status: verifier.StatusBroken, reason: verifier.ReasonPrevHashMismatch, seq: 3,
		},
		{
			name: "forge the head hash",
			attack: func(t *testing.T, e verifyEnv) {
				e.exec(t, `UPDATE audit_chain_heads SET head_hash = decode(repeat('ab', 32), 'hex') WHERE org_id = $1`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonHeadHashMismatch, seq: 10,
		},
		{
			name: "raise the head past the rows",
			attack: func(t *testing.T, e verifyEnv) {
				e.exec(t, `UPDATE audit_chain_heads SET head_seq = 12 WHERE org_id = $1`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonTailMissing, seq: 11,
		},
		{
			name: "rewrite the tail and recompute, keeping the attestations",
			attack: func(t *testing.T, e verifyEnv) {
				e.rechainFrom(t, 7, "denied")
			},
			status: verifier.StatusBroken, reason: verifier.ReasonAttestationMismatch, seq: 10,
		},
		{
			name: "rewrite the whole chain without the key",
			attack: func(t *testing.T, e verifyEnv) {
				e.rechainFrom(t, 2, "denied")
			},
			status: verifier.StatusBroken, reason: verifier.ReasonAttestationMismatch, seq: 5,
		},
		{
			// With the attestations gone nothing signed contradicts the rewrite.
			// The verdict must still not be "verified".
			name: "rewrite the whole chain and delete the attestations",
			attack: func(t *testing.T, e verifyEnv) {
				e.rechainFrom(t, 2, "denied")
				e.exec(t, `DELETE FROM audit_chain_attestations WHERE org_id = $1`, e.org)
			},
			status: verifier.StatusPartial, reason: verifier.ReasonNoAttestation, seq: 0,
		},
		{
			name: "forge the newest attestation with another key",
			attack: func(t *testing.T, e verifyEnv) {
				var hash []byte
				var signedAt time.Time
				if err := e.pool.QueryRow(e.ctx, `SELECT row_hash, signed_at FROM audit_chain_attestations
					WHERE org_id = $1 AND chain_seq = 10`, e.org).Scan(&hash, &signedAt); err != nil {
					t.Fatal(err)
				}
				rogue := pruneRing(t, "audit-test-a", 9, "")
				_, sig, err := rogue.SignAttestation(e.org, 10, hash, signedAt)
				if err != nil {
					t.Fatal(err)
				}
				e.exec(t, `UPDATE audit_chain_attestations SET signature = $2 WHERE org_id = $1 AND chain_seq = 10`, e.org, sig)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonAttestationSignature, seq: 10,
		},
	}
	for _, tc := range cases {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			e := newAttestedChain(t, "attack")
			tc.attack(t, e)
			assertReport(t, e.verify(t, verifier.Options{}), tc.status, tc.reason, tc.seq)
		})
	}
}

// newPrunedChain inserts rows 1..20, ages 1..10, attests the head and purges, so
// the chain starts at a signed checkpoint through 10.
func newPrunedChain(t *testing.T, label string) verifyEnv {
	t.Helper()
	e := newVerifyEnv(t, label)
	insertChained(t, e.ctx, e.st, e.org, 1, 20)
	backdateAndRechain(t, e.ctx, e.pool, e.org, 10)
	if _, err := e.st.Purge(e.ctx, 365); err != nil {
		t.Fatalf("Purge: %v", err)
	}
	e.attest(t)
	return e
}

func TestVerifyOrgPrunedChainStartsAtItsCheckpoint(t *testing.T) {
	e := newPrunedChain(t, "pruned")
	rep := e.verify(t, verifier.Options{})
	assertReport(t, rep, verifier.StatusVerified, "", 0)
	if rep.FromSeq != 11 || rep.RowsVerified != 10 || rep.Checkpoint == nil || !rep.Checkpoint.Verified || rep.Checkpoint.ThroughSeq != 10 {
		t.Fatalf("report = %+v", rep)
	}
}

func TestVerifyOrgFullyPrunedChainIsVerifiedAtItsCheckpoint(t *testing.T) {
	e := newVerifyEnv(t, "allpruned")
	insertChained(t, e.ctx, e.st, e.org, 1, 6)
	backdateAndRechain(t, e.ctx, e.pool, e.org, 6)
	if _, err := e.st.Purge(e.ctx, 365); err != nil {
		t.Fatal(err)
	}
	e.attest(t)
	rep := e.verify(t, verifier.Options{})
	assertReport(t, rep, verifier.StatusVerified, "", 0)
	if rep.RowsVerified != 0 || rep.ThroughSeq != 6 {
		t.Fatalf("report = %+v", rep)
	}
}

func TestVerifyOrgCheckpointAttacks(t *testing.T) {
	cases := []struct {
		name   string
		attack func(t *testing.T, e verifyEnv)
		status verifier.Status
		reason string
		seq    int64
	}{
		{
			name: "drop the checkpoint",
			attack: func(t *testing.T, e verifyEnv) {
				e.exec(t, `DELETE FROM audit_chain_checkpoints WHERE org_id = $1`, e.org)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonPrefixMissing, seq: 1,
		},
		{
			name: "forge the checkpoint with another key under the real key id",
			attack: func(t *testing.T, e verifyEnv) {
				cp := loadCheckpoints(t, e.ctx, e.pool, e.org)[0]
				rogue := pruneRing(t, "audit-test-a", 9, "")
				_, sig, err := rogue.SignCheckpoint(e.org, cp.throughSeq, cp.hash, cp.total, cp.createdAt)
				if err != nil {
					t.Fatal(err)
				}
				e.exec(t, `UPDATE audit_chain_checkpoints SET signature = $2 WHERE org_id = $1`, e.org, sig)
			},
			status: verifier.StatusBroken, reason: verifier.ReasonCheckpointSignature, seq: 10,
		},
		{
			// A key this verifier does not hold cannot be told from a forger, but
			// it is not "broken": the anchor is unverifiable.
			name: "checkpoint signed under a key id the verifier does not know",
			attack: func(t *testing.T, e verifyEnv) {
				cp := loadCheckpoints(t, e.ctx, e.pool, e.org)[0]
				rogue := pruneRing(t, "rogue-key", 9, "")
				keyID, sig, err := rogue.SignCheckpoint(e.org, cp.throughSeq, cp.hash, cp.total, cp.createdAt)
				if err != nil {
					t.Fatal(err)
				}
				e.exec(t, `UPDATE audit_chain_checkpoints SET key_id = $2, signature = $3 WHERE org_id = $1`, e.org, keyID, sig)
			},
			status: verifier.StatusKeyUnavailable, reason: verifier.ReasonKeyUnavailable, seq: 0,
		},
	}
	for _, tc := range cases {
		tc := tc
		t.Run(tc.name, func(t *testing.T) {
			e := newPrunedChain(t, "ckptattack")
			tc.attack(t, e)
			assertReport(t, e.verify(t, verifier.Options{}), tc.status, tc.reason, tc.seq)
		})
	}
}

func TestVerifyOrgReportsLegacyRowsAndNeverHidesThem(t *testing.T) {
	ctx, pool := chainDB(t)
	ensureVerificationState(t, ctx, pool)
	org := verifyOrg(t, pool, "legacy")
	ring := pruneRing(t, "audit-test-a", 1, "")

	unchained := store.New(pool)
	insertChained(t, ctx, unchained, org, 1, 2) // written before the chain was on
	chained := store.New(pool, store.WithChain(), store.WithChainKeys(ring))
	insertChained(t, ctx, chained, org, 3, 6)
	if _, err := chained.Attest(ctx); err != nil {
		t.Fatal(err)
	}

	rep, err := chained.VerifyOrg(ctx, org, verifier.Options{})
	if err != nil {
		t.Fatal(err)
	}
	assertReport(t, rep, verifier.StatusVerifiedWithLegacy, "", 0)
	if rep.LegacyUnchainedRows != 2 || rep.RowsVerified != 4 {
		t.Fatalf("report = %+v, want 2 legacy rows and 4 verified", rep)
	}
}

func TestVerifyOrgWithoutAChainIsPartialNoChain(t *testing.T) {
	ctx, pool := chainDB(t)
	org := verifyOrg(t, pool, "nochain")
	insertChained(t, ctx, store.New(pool), org, 1, 3)
	rep, err := store.New(pool, store.WithChain()).VerifyOrg(ctx, org, verifier.Options{})
	if err != nil {
		t.Fatal(err)
	}
	assertReport(t, rep, verifier.StatusPartial, verifier.ReasonNoChain, 0)
	if rep.LegacyUnchainedRows != 3 {
		t.Fatalf("legacy = %d, want 3", rep.LegacyUnchainedRows)
	}
}

func TestVerifyOrgRowLimitIsPartialAndTheCursorResumes(t *testing.T) {
	e := newAttestedChain(t, "resume")
	first := e.verify(t, verifier.Options{MaxRows: 4})
	assertReport(t, first, verifier.StatusPartial, verifier.ReasonRowLimit, 0)
	if first.ThroughSeq != 4 || first.Complete || len(first.ThroughHash) != chain.HashSize {
		t.Fatalf("first run = %+v", first)
	}

	resumed := e.verify(t, verifier.Options{FromSeq: first.ThroughSeq, FromHash: first.ThroughHash})
	assertReport(t, resumed, verifier.StatusVerified, "", 0)
	if resumed.FromSeq != 4 || resumed.RowsVerified != 7 {
		t.Fatalf("resumed run = %+v", resumed)
	}

	// A change inside the range the cursor already covers is caught at the
	// cursor row itself.
	e.rechainFrom(t, 4, "denied")
	assertReport(t, e.verify(t, verifier.Options{FromSeq: first.ThroughSeq, FromHash: first.ThroughHash}),
		verifier.StatusBroken, verifier.ReasonResumeAnchor, 4)
}

func TestVerifierStateRoundTrips(t *testing.T) {
	e := newVerifyEnv(t, "state")
	empty, err := e.st.LoadVerifierState(e.ctx, e.org)
	if err != nil || empty.Status != "" || !empty.LastRunAt.IsZero() {
		t.Fatalf("state before any run = %+v err %v", empty, err)
	}

	want := verifier.State{
		Status: verifier.StatusBroken, Reason: verifier.ReasonRowHashMismatch, FirstBrokenSeq: 8,
		ThroughSeq: 7, ThroughHash: bytes.Repeat([]byte{7}, chain.HashSize), LegacyUnchainedRows: 2,
		LastRunAt:     time.Now().UTC().Truncate(time.Microsecond),
		LastFullAt:    time.Now().UTC().Add(-time.Hour).Truncate(time.Microsecond),
		FullCursorSeq: 5, FullCursorHash: bytes.Repeat([]byte{5}, chain.HashSize),
	}
	if err := e.st.SaveVerifierState(e.ctx, e.org, want); err != nil {
		t.Fatal(err)
	}
	got, err := e.st.LoadVerifierState(e.ctx, e.org)
	if err != nil {
		t.Fatal(err)
	}
	if got.Status != want.Status || got.Reason != want.Reason || got.FirstBrokenSeq != 8 || got.ThroughSeq != 7 ||
		!bytes.Equal(got.ThroughHash, want.ThroughHash) || got.LegacyUnchainedRows != 2 ||
		!got.LastRunAt.Equal(want.LastRunAt) || !got.LastFullAt.Equal(want.LastFullAt) ||
		got.FullCursorSeq != 5 || !bytes.Equal(got.FullCursorHash, want.FullCursorHash) {
		t.Fatalf("state = %+v, want %+v", got, want)
	}

	// Overwriting with a clean state clears the optional fields.
	clean := verifier.State{Status: verifier.StatusVerified, LastRunAt: want.LastRunAt}
	if err := e.st.SaveVerifierState(e.ctx, e.org, clean); err != nil {
		t.Fatal(err)
	}
	got, err = e.st.LoadVerifierState(e.ctx, e.org)
	if err != nil || got.FirstBrokenSeq != 0 || !got.LastFullAt.IsZero() || got.FullCursorSeq != 0 {
		t.Fatalf("state after overwrite = %+v err %v", got, err)
	}
}

func TestVerifierPassRecordsStateAndCountsABrokenChain(t *testing.T) {
	e := newAttestedChain(t, "pass")
	cfg := verifier.Config{Interval: time.Hour}

	if err := verifier.Pass(e.ctx, e.st, cfg); err != nil {
		t.Fatalf("Pass: %v", err)
	}
	st, err := e.st.LoadVerifierState(e.ctx, e.org)
	if err != nil {
		t.Fatal(err)
	}
	if st.Status != verifier.StatusVerified || st.ThroughSeq != 10 || st.LastFullAt.IsZero() {
		t.Fatalf("state after a clean pass = %+v", st)
	}

	before := counterValue(t, "audit_core_chain_verify_failures_total", nil)
	e.maintenance(t, `UPDATE audit_events SET outcome = 'denied' WHERE org_id = $1 AND chain_seq = 3`, e.org)
	// The next pass is incremental; the weekly full pass is what covers row 3.
	e.exec(t, `UPDATE audit_chain_verifications SET last_full_at = NULL WHERE org_id = $1`, e.org)
	if err := verifier.Pass(e.ctx, e.st, cfg); err != nil {
		t.Fatalf("Pass: %v", err)
	}
	st, err = e.st.LoadVerifierState(e.ctx, e.org)
	if err != nil {
		t.Fatal(err)
	}
	if st.Status != verifier.StatusBroken || st.FirstBrokenSeq != 3 || st.Reason != verifier.ReasonRowHashMismatch || st.ThroughSeq != 10 {
		t.Fatalf("state after tamper = %+v", st)
	}
	if after := counterValue(t, "audit_core_chain_verify_failures_total", nil); after <= before {
		t.Fatalf("verify failures counter %v -> %v, want it to rise", before, after)
	}
}
