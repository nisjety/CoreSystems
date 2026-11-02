package store_test

import (
	"bytes"
	"context"
	"crypto/ed25519"
	"fmt"
	"testing"
	"time"

	"github.com/jackc/pgx/v5/pgxpool"

	"github.com/triodelab/controlplane/audit-core/internal/chain"
	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// These tests need Postgres (TEST_DATABASE_URL) like the other chain tests and
// skip without it. compliance_test.go proves each compliance read on its happy
// path; this file pins the exact values the compliance SQL returns at the edges:
// window bounds, each filter column alone and combined, page boundaries, two
// orgs in one table, the manifest of an empty or never-chained range, and the
// keys one org's anchors used.

// dbBase is the origin of the ingested_at timeline these tests lay down.
var dbBase = time.Date(2026, 10, 1, 0, 0, 0, 0, time.UTC)

// minuteAt is the instant m minutes after dbBase. Whole minutes keep every
// window edge exact to the microsecond the column stores.
func minuteAt(m int) time.Time {
	return dbBase.Add(time.Duration(m) * time.Minute)
}

// restampIngestedAt gives the org's chained rows the ingest instants at(seq) and
// recomputes every hash from the first row on, the way backdateAndRechain does.
// ingested_at is part of the row hash, so a plain UPDATE would break the chain.
// Run it before any Attest, which signs the head hash this replaces.
func restampIngestedAt(t *testing.T, ctx context.Context, pool *pgxpool.Pool, org string, at func(seq int64) time.Time) {
	t.Helper()
	rows := loadChain(t, ctx, pool, org)
	if len(rows) == 0 {
		t.Fatalf("org %s has no chained rows to restamp", org)
	}
	prev := rows[0].prev
	for i := range rows {
		r := &rows[i]
		r.row.IngestedAt = at(r.seq).UTC().Truncate(time.Microsecond)
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
	assertChainValid(t, org, loadChain(t, ctx, pool, org))
}

// onePage reads one page of a filter and fails the test on error.
func onePage(t *testing.T, ctx context.Context, st *store.Store, f store.ComplianceFilter) store.CompliancePage {
	t.Helper()
	page, err := st.ComplianceEvents(ctx, f)
	if err != nil {
		t.Fatalf("ComplianceEvents(%+v): %v", f, err)
	}
	return page
}

// manifestOf reads one manifest and fails the test on error.
func manifestOf(t *testing.T, ctx context.Context, st *store.Store, org string, since, until time.Time) store.ComplianceManifest {
	t.Helper()
	m, err := st.ComplianceManifest(ctx, org, since, until)
	if err != nil {
		t.Fatalf("ComplianceManifest(%s): %v", org, err)
	}
	return m
}

// seqList is the chain_seq of each event, in order.
func seqList(got []store.ComplianceEvent) []int64 {
	seqs := make([]int64, 0, len(got))
	for _, e := range got {
		seqs = append(seqs, e.ChainSeq)
	}
	return seqs
}

// assertSeqs fails unless the events carry exactly the chain_seq values want, in order.
func assertSeqs(t *testing.T, got []store.ComplianceEvent, want ...int64) {
	t.Helper()
	if len(got) != len(want) {
		t.Fatalf("chain_seq list = %v, want %v", seqList(got), want)
	}
	for i := range want {
		if got[i].ChainSeq != want[i] {
			t.Fatalf("chain_seq list = %v, want %v", seqList(got), want)
		}
	}
}

// requireEmptyPage fails unless the read finds nothing: an empty list (never nil),
// no cursor and no error.
func requireEmptyPage(t *testing.T, ctx context.Context, st *store.Store, f store.ComplianceFilter) {
	t.Helper()
	page := onePage(t, ctx, st, f)
	if page.Events == nil || len(page.Events) != 0 || page.NextSeq != 0 {
		t.Fatalf("page has %d events (nil list: %v) and cursor %d, want an empty list and no cursor",
			len(page.Events), page.Events == nil, page.NextSeq)
	}
}

// assertIngestedAt fails unless every event carries the ingest instant the test
// gave its chain_seq, which shows the window was read against those instants.
func assertIngestedAt(t *testing.T, got []store.ComplianceEvent, at func(seq int64) time.Time) {
	t.Helper()
	for _, e := range got {
		if want := at(e.ChainSeq); !e.IngestedAt.Equal(want) {
			t.Fatalf("seq %d ingested_at %s, want %s", e.ChainSeq, e.IngestedAt, want)
		}
	}
}

// requireKeys fails unless the listing is exactly want, in order.
func requireKeys(t *testing.T, got []store.ComplianceKey, want ...store.ComplianceKey) {
	t.Helper()
	if len(got) != len(want) {
		t.Fatalf("keys = %+v, want %+v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("keys = %+v, want %+v", got, want)
		}
	}
}

// filterRow is the part of one chained row the filter tests vary.
type filterRow struct {
	event, user, plane, outcome string
}

// insertFilterRows appends one chained row per entry. Row i+1 keeps the
// chainEvent identity chain:<org>:<i+1>, so a returned event names its seed.
func insertFilterRows(t *testing.T, ctx context.Context, st *store.Store, org string, rows []filterRow) {
	t.Helper()
	for i, r := range rows {
		ev := chainEvent(org, i+1)
		ev.Event, ev.UserID, ev.Plane, ev.Outcome = r.event, r.user, r.plane, r.outcome
		if _, err := st.InsertAudit(ctx, ev); err != nil {
			t.Fatalf("insert %d: %v", i+1, err)
		}
	}
}

// TestComplianceDBWindowResolution proves the ingested_at window on eight rows
// with one instant each: both ends are inclusive, a window between rows takes
// the rows inside it, and a window that holds no row, starts after the newest,
// ends before the oldest, or starts after its end comes back as an empty list.
func TestComplianceDBWindowResolution(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "db-window")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(pruneRing(t, "audit-test-a", 1, "")))
	insertChained(t, ctx, st, org, 1, 8)
	ingestAt := func(seq int64) time.Time { return minuteAt(int(seq)) }
	restampIngestedAt(t, ctx, pool, org, ingestAt)
	env := verifyEnv{ctx: ctx, pool: pool, st: st, org: org}

	t.Run("both ends are inclusive", func(t *testing.T) {
		got := pageAll(t, env, store.ComplianceFilter{OrgID: org, Since: minuteAt(3), Until: minuteAt(6)})
		assertSeqs(t, got, 3, 4, 5, 6)
		assertIngestedAt(t, got, ingestAt)
	})
	t.Run("a single instant selects the row ingested at it", func(t *testing.T) {
		assertSeqs(t, pageAll(t, env, store.ComplianceFilter{OrgID: org, Since: minuteAt(5), Until: minuteAt(5)}), 5)
	})
	t.Run("a window between two rows takes the rows inside it", func(t *testing.T) {
		got := pageAll(t, env, store.ComplianceFilter{
			OrgID: org, Since: minuteAt(3).Add(30 * time.Second), Until: minuteAt(5).Add(30 * time.Second),
		})
		assertSeqs(t, got, 4, 5)
		assertIngestedAt(t, got, ingestAt)
	})
	t.Run("a window that holds no row is empty", func(t *testing.T) {
		requireEmptyPage(t, ctx, st, store.ComplianceFilter{
			OrgID: org, Since: minuteAt(5).Add(10 * time.Second), Until: minuteAt(5).Add(50 * time.Second),
		})
	})
	t.Run("a window that starts after the newest row is empty", func(t *testing.T) {
		requireEmptyPage(t, ctx, st, store.ComplianceFilter{OrgID: org, Since: minuteAt(8).Add(time.Second)})
	})
	t.Run("a window that ends before the oldest row is empty", func(t *testing.T) {
		requireEmptyPage(t, ctx, st, store.ComplianceFilter{OrgID: org, Until: minuteAt(1).Add(-time.Second)})
	})
	t.Run("a window whose start is after its end is empty", func(t *testing.T) {
		requireEmptyPage(t, ctx, st, store.ComplianceFilter{OrgID: org, Since: minuteAt(6), Until: minuteAt(2)})
	})
	t.Run("an open start reads from the first row", func(t *testing.T) {
		assertSeqs(t, pageAll(t, env, store.ComplianceFilter{OrgID: org, Until: minuteAt(4)}), 1, 2, 3, 4)
	})
	t.Run("an open end reads to the newest row", func(t *testing.T) {
		assertSeqs(t, pageAll(t, env, store.ComplianceFilter{OrgID: org, Since: minuteAt(6)}), 6, 7, 8)
	})
	t.Run("bounds past both ends read every row", func(t *testing.T) {
		got := pageAll(t, env, store.ComplianceFilter{OrgID: org, Since: minuteAt(0), Until: minuteAt(100)})
		assertSeqs(t, got, 1, 2, 3, 4, 5, 6, 7, 8)
	})
}

// TestComplianceDBWindowIsContiguousWhenIngestIsNotOrdered pins the documented
// contract for ingest times that do not follow chain_seq. The window finds only
// the two ends: the lowest chain_seq ingested at or after the start and the
// highest ingested at or before the end. Everything between them is returned, so
// the range stays one gap-free chain_seq run.
func TestComplianceDBWindowIsContiguousWhenIngestIsNotOrdered(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "db-window-order")
	st := store.New(pool, store.WithChain())
	insertChained(t, ctx, st, org, 1, 8)
	// seq 2 was committed late, at minute 9; seq 3 and 4 were ingested early.
	ingested := []int{1, 9, 3, 4, 5, 6, 7, 8}
	ingestAt := func(seq int64) time.Time { return minuteAt(ingested[seq-1]) }
	restampIngestedAt(t, ctx, pool, org, ingestAt)

	// The window [3:30, 4:30] holds only seq 4. Its ends are seq 2 (the lowest
	// chain_seq ingested after the start) and seq 4 (the highest ingested before
	// the end), so seq 3, ingested before the window, is returned between them.
	got := pageAll(t, verifyEnv{ctx: ctx, pool: pool, st: st, org: org},
		store.ComplianceFilter{OrgID: org, Since: minuteAt(3).Add(30 * time.Second), Until: minuteAt(4).Add(30 * time.Second)})
	assertSeqs(t, got, 2, 3, 4)
	assertIngestedAt(t, got, ingestAt)
}

// TestComplianceDBKeysetPaging walks eight rows with several page sizes. Each
// page resumes after its cursor in chain_seq order, the last page carries no
// cursor, and a page size out of range reads everything.
func TestComplianceDBKeysetPaging(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "db-paging")
	st := store.New(pool, store.WithChain())
	insertChained(t, ctx, st, org, 1, 8)

	t.Run("pages of three walk the rows once each, in order", func(t *testing.T) {
		p1 := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, Limit: 3})
		assertSeqs(t, p1.Events, 1, 2, 3)
		if p1.NextSeq != 3 || p1.HeadSeq != 8 {
			t.Fatalf("page 1 cursor %d head %d, want cursor 3 head 8", p1.NextSeq, p1.HeadSeq)
		}
		p2 := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, Limit: 3, AfterSeq: p1.NextSeq})
		assertSeqs(t, p2.Events, 4, 5, 6)
		if p2.NextSeq != 6 {
			t.Fatalf("page 2 cursor %d, want 6", p2.NextSeq)
		}
		p3 := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, Limit: 3, AfterSeq: p2.NextSeq})
		assertSeqs(t, p3.Events, 7, 8)
		if p3.NextSeq != 0 {
			t.Fatalf("last page cursor %d, want none", p3.NextSeq)
		}
	})
	t.Run("a page size that divides the count ends on a full page with no cursor", func(t *testing.T) {
		p1 := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, Limit: 4})
		assertSeqs(t, p1.Events, 1, 2, 3, 4)
		if p1.NextSeq != 4 {
			t.Fatalf("page 1 cursor %d, want 4", p1.NextSeq)
		}
		p2 := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, Limit: 4, AfterSeq: p1.NextSeq})
		assertSeqs(t, p2.Events, 5, 6, 7, 8)
		if p2.NextSeq != 0 {
			t.Fatalf("last page cursor %d, want none", p2.NextSeq)
		}
	})
	t.Run("a page of one visits every row, and only the last page has no cursor", func(t *testing.T) {
		var cursor int64
		for seq := int64(1); seq <= 8; seq++ {
			page := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, Limit: 1, AfterSeq: cursor})
			assertSeqs(t, page.Events, seq)
			wantCursor := seq
			if seq == 8 {
				wantCursor = 0
			}
			if page.NextSeq != wantCursor {
				t.Fatalf("seq %d page cursor %d, want %d", seq, page.NextSeq, wantCursor)
			}
			cursor = page.NextSeq
		}
	})
	t.Run("a page size that covers the rows, or is out of range, returns them all", func(t *testing.T) {
		for _, limit := range []int{0, -1, 8, 9, 1000, 5000} {
			page := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, Limit: limit})
			assertSeqs(t, page.Events, 1, 2, 3, 4, 5, 6, 7, 8)
			if page.NextSeq != 0 {
				t.Fatalf("limit %d: cursor %d, want none", limit, page.NextSeq)
			}
		}
	})
	t.Run("a cursor at or past the newest row returns an empty page at the head", func(t *testing.T) {
		for _, after := range []int64{8, 9, 100} {
			page := onePage(t, ctx, st, store.ComplianceFilter{OrgID: org, AfterSeq: after})
			if len(page.Events) != 0 || page.NextSeq != 0 || page.HeadSeq != 8 {
				t.Fatalf("after %d: %d events, cursor %d, head %d; want none, no cursor, head 8",
					after, len(page.Events), page.NextSeq, page.HeadSeq)
			}
		}
	})
}

// TestComplianceDBFilters proves each thinning column alone and in combination
// against exact chain_seq sets, that matching is exact (no prefix, no case
// folding), that an empty value means no filter, and that a filtered walk pages
// over the matching rows only.
func TestComplianceDBFilters(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "db-filters")
	st := store.New(pool, store.WithChain())
	insertFilterRows(t, ctx, st, org, []filterRow{
		{event: "login", user: "alice", plane: "control", outcome: "ok"},           // 1
		{event: "login", user: "bob", plane: "control", outcome: "denied"},         // 2
		{event: "tool_call", user: "alice", plane: "application", outcome: "ok"},   // 3
		{event: "tool_call", user: "bob", plane: "application", outcome: "denied"}, // 4
		{event: "login", user: "alice", plane: "control", outcome: "denied"},       // 5
		{event: "tool_call", user: "alice", plane: "data", outcome: "ok"},          // 6
		{event: "approval", user: "carol", plane: "data", outcome: "ok"},           // 7
		{event: "login", user: "bob", plane: "application", outcome: "ok"},         // 8
	})
	env := verifyEnv{ctx: ctx, pool: pool, st: st, org: org}

	cases := []struct {
		name   string
		filter store.ComplianceFilter
		want   []int64
	}{
		{"event alone", store.ComplianceFilter{Event: "login"}, []int64{1, 2, 5, 8}},
		{"event alone, a second value", store.ComplianceFilter{Event: "tool_call"}, []int64{3, 4, 6}},
		{"event alone, a single row", store.ComplianceFilter{Event: "approval"}, []int64{7}},
		{"user alone", store.ComplianceFilter{UserID: "alice"}, []int64{1, 3, 5, 6}},
		{"user alone, a second value", store.ComplianceFilter{UserID: "bob"}, []int64{2, 4, 8}},
		{"user alone, a single row", store.ComplianceFilter{UserID: "carol"}, []int64{7}},
		{"plane alone", store.ComplianceFilter{Plane: "control"}, []int64{1, 2, 5}},
		{"plane alone, application", store.ComplianceFilter{Plane: "application"}, []int64{3, 4, 8}},
		{"plane alone, data", store.ComplianceFilter{Plane: "data"}, []int64{6, 7}},
		{"outcome alone, ok", store.ComplianceFilter{Outcome: "ok"}, []int64{1, 3, 6, 7, 8}},
		{"outcome alone, denied", store.ComplianceFilter{Outcome: "denied"}, []int64{2, 4, 5}},
		{"event and outcome", store.ComplianceFilter{Event: "login", Outcome: "denied"}, []int64{2, 5}},
		{"event and outcome, ok", store.ComplianceFilter{Event: "login", Outcome: "ok"}, []int64{1, 8}},
		{"user and plane", store.ComplianceFilter{UserID: "alice", Plane: "control"}, []int64{1, 5}},
		{"user and plane, bob", store.ComplianceFilter{UserID: "bob", Plane: "application"}, []int64{4, 8}},
		{"event, user and outcome", store.ComplianceFilter{Event: "login", UserID: "bob", Outcome: "denied"}, []int64{2}},
		{"all four columns", store.ComplianceFilter{Event: "tool_call", UserID: "alice", Plane: "data", Outcome: "ok"}, []int64{6}},
		{"two columns that match no row together", store.ComplianceFilter{Event: "login", Plane: "data"}, []int64{}},
		{"no column set", store.ComplianceFilter{}, []int64{1, 2, 3, 4, 5, 6, 7, 8}},
		{"every column empty", store.ComplianceFilter{Event: "", UserID: "", Plane: "", Outcome: ""}, []int64{1, 2, 3, 4, 5, 6, 7, 8}},
		{"event is an exact match, not a prefix", store.ComplianceFilter{Event: "log"}, []int64{}},
		{"event is case sensitive", store.ComplianceFilter{Event: "LOGIN"}, []int64{}},
		{"plane is case sensitive", store.ComplianceFilter{Plane: "Control"}, []int64{}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			f := tc.filter
			f.OrgID = org
			assertSeqs(t, pageAll(t, env, f), tc.want...)
		})
	}

	t.Run("a filtered walk pages over the matching rows only", func(t *testing.T) {
		f := store.ComplianceFilter{OrgID: org, Event: "login", Limit: 2}
		p1 := onePage(t, ctx, st, f)
		assertSeqs(t, p1.Events, 1, 2)
		if p1.NextSeq != 2 {
			t.Fatalf("filtered page 1 cursor %d, want 2", p1.NextSeq)
		}
		f.AfterSeq = p1.NextSeq
		p2 := onePage(t, ctx, st, f)
		assertSeqs(t, p2.Events, 5, 8)
		if p2.NextSeq != 0 {
			t.Fatalf("filtered page 2 cursor %d, want none", p2.NextSeq)
		}
	})
}

// TestComplianceDBOrgIsolation puts two orgs in one table. Both chains start at
// chain_seq 1 and their ingest times interleave, so a read that dropped org_id
// would mix two sequences or two windows. No read may return the other org's
// rows, its head or its counts.
func TestComplianceDBOrgIsolation(t *testing.T) {
	ctx, pool := chainDB(t)
	st := store.New(pool, store.WithChain(), store.WithChainKeys(pruneRing(t, "audit-test-a", 1, "")))
	orgA := pruneOrg(t, pool, "db-iso-a")
	orgB := pruneOrg(t, pool, "db-iso-b")
	insertChained(t, ctx, st, orgA, 1, 8)
	insertChained(t, ctx, st, orgB, 1, 10)
	// Org A is ingested on even minutes and org B on odd ones.
	restampIngestedAt(t, ctx, pool, orgA, func(seq int64) time.Time { return minuteAt(int(2 * seq)) })
	restampIngestedAt(t, ctx, pool, orgB, func(seq int64) time.Time { return minuteAt(int(2*seq + 1)) })
	// One row of org B was written while the chain was off: it has no position.
	if _, err := store.New(pool).InsertAudit(ctx, chainEvent(orgB, 99)); err != nil {
		t.Fatalf("insert unchained row: %v", err)
	}
	rowsA := loadChain(t, ctx, pool, orgA)
	rowsB := loadChain(t, ctx, pool, orgB)
	envA := verifyEnv{ctx: ctx, pool: pool, st: st, org: orgA}
	envB := verifyEnv{ctx: ctx, pool: pool, st: st, org: orgB}

	t.Run("events hold only the org's own rows and its own head", func(t *testing.T) {
		a := pageAll(t, envA, store.ComplianceFilter{OrgID: orgA})
		assertSeqs(t, a, 1, 2, 3, 4, 5, 6, 7, 8)
		for _, e := range a {
			if e.OrgID != orgA || e.EventID != fmt.Sprintf("chain:%s:%d", orgA, e.ChainSeq) {
				t.Fatalf("org A read returned org %s seq %d event %s", e.OrgID, e.ChainSeq, e.EventID)
			}
		}
		b := pageAll(t, envB, store.ComplianceFilter{OrgID: orgB})
		assertSeqs(t, b, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
		for _, e := range b {
			if e.OrgID != orgB || e.EventID != fmt.Sprintf("chain:%s:%d", orgB, e.ChainSeq) {
				t.Fatalf("org B read returned org %s seq %d event %s", e.OrgID, e.ChainSeq, e.EventID)
			}
		}
		if head := onePage(t, ctx, st, store.ComplianceFilter{OrgID: orgA}).HeadSeq; head != 8 {
			t.Fatalf("org A head %d, want 8", head)
		}
		if head := onePage(t, ctx, st, store.ComplianceFilter{OrgID: orgB}).HeadSeq; head != 10 {
			t.Fatalf("org B head %d, want 10", head)
		}
	})
	t.Run("a window reads only its own org's rows", func(t *testing.T) {
		// Org A is ingested at minutes 2, 4, 6, 8, so [5, 9] holds seq 3 and 4.
		// Org B is ingested at minutes 3, 5, 7, 9, so [5, 9] holds seq 2, 3 and 4.
		since, until := minuteAt(5), minuteAt(9)
		assertSeqs(t, pageAll(t, envA, store.ComplianceFilter{OrgID: orgA, Since: since, Until: until}), 3, 4)
		assertSeqs(t, pageAll(t, envB, store.ComplianceFilter{OrgID: orgB, Since: since, Until: until}), 2, 3, 4)
	})
	t.Run("a manifest counts its own org's chain, legacy rows and head", func(t *testing.T) {
		ma := manifestOf(t, ctx, st, orgA, time.Time{}, time.Time{})
		if ma.FirstSeq != 1 || ma.LastSeq != 8 || ma.Count != 8 || ma.HeadSeq != 8 || ma.LegacyUnchainedRows != 0 {
			t.Fatalf("org A manifest = %+v", ma)
		}
		if !bytes.Equal(ma.LastRowHash, rowsA[7].hash) || !bytes.Equal(ma.HeadHash, rowsA[7].hash) {
			t.Fatal("org A manifest does not end on org A's last row")
		}
		mb := manifestOf(t, ctx, st, orgB, time.Time{}, time.Time{})
		if mb.FirstSeq != 1 || mb.LastSeq != 10 || mb.Count != 10 || mb.HeadSeq != 10 || mb.LegacyUnchainedRows != 1 {
			t.Fatalf("org B manifest = %+v", mb)
		}
		if !bytes.Equal(mb.LastRowHash, rowsB[9].hash) || !bytes.Equal(mb.HeadHash, rowsB[9].hash) {
			t.Fatal("org B manifest does not end on org B's last row")
		}
	})
	t.Run("a window manifest counts only its org's rows in the window", func(t *testing.T) {
		ma := manifestOf(t, ctx, st, orgA, minuteAt(5), minuteAt(9))
		if ma.FirstSeq != 3 || ma.LastSeq != 4 || ma.Count != 2 || ma.HeadSeq != 8 {
			t.Fatalf("org A window manifest = %+v", ma)
		}
		mb := manifestOf(t, ctx, st, orgB, minuteAt(5), minuteAt(9))
		if mb.FirstSeq != 2 || mb.LastSeq != 4 || mb.Count != 3 || mb.HeadSeq != 10 {
			t.Fatalf("org B window manifest = %+v", mb)
		}
	})
	t.Run("an empty org id is refused rather than read as every org", func(t *testing.T) {
		if _, err := st.ComplianceEvents(ctx, store.ComplianceFilter{Event: "chain_test"}); err == nil {
			t.Fatal("ComplianceEvents without an org id returned no error")
		}
		if _, err := st.ComplianceManifest(ctx, "", time.Time{}, time.Time{}); err == nil {
			t.Fatal("ComplianceManifest without an org id returned no error")
		}
	})
}

// TestComplianceDBManifestEnds checks a manifest's ends, counts and anchors on
// rows the insert path wrote, and what it is signed with: a test signer, the key
// ring it was built with, or nothing when no key is held.
func TestComplianceDBManifestEnds(t *testing.T) {
	ctx, pool := chainDB(t)
	ring := pruneRing(t, "audit-test-a", 1, "")
	org := pruneOrg(t, pool, "db-manifest")
	st := store.New(pool, store.WithChain(), store.WithChainKeys(ring))
	insertChained(t, ctx, st, org, 1, 8)
	restampIngestedAt(t, ctx, pool, org, func(seq int64) time.Time { return minuteAt(int(seq)) })
	rows := loadChain(t, ctx, pool, org)
	genesis, err := chain.GenesisHash(org)
	if err != nil {
		t.Fatal(err)
	}

	t.Run("the whole chain reports its ends, count and head", func(t *testing.T) {
		m := manifestOf(t, ctx, st, org, time.Time{}, time.Time{})
		if m.FirstSeq != 1 || m.LastSeq != 8 || m.Count != 8 || m.HeadSeq != 8 || m.LegacyUnchainedRows != 0 {
			t.Fatalf("manifest = %+v", m)
		}
		if !bytes.Equal(m.FirstPrevHash, genesis) || !bytes.Equal(m.LastRowHash, rows[7].hash) || !bytes.Equal(m.HeadHash, rows[7].hash) {
			t.Fatal("manifest hashes are not the genesis link and the last row")
		}
		if m.Checkpoint != nil || m.Attestation != nil {
			t.Fatalf("an org never pruned or attested has anchors: checkpoint %+v attestation %+v", m.Checkpoint, m.Attestation)
		}
	})
	t.Run("a window reports its own ends and still the real head", func(t *testing.T) {
		m := manifestOf(t, ctx, st, org, minuteAt(3), minuteAt(6))
		if m.FirstSeq != 3 || m.LastSeq != 6 || m.Count != 4 || m.HeadSeq != 8 {
			t.Fatalf("window manifest = %+v", m)
		}
		if !bytes.Equal(m.FirstPrevHash, rows[1].hash) || !bytes.Equal(m.LastRowHash, rows[5].hash) || !bytes.Equal(m.HeadHash, rows[7].hash) {
			t.Fatal("window manifest hashes are not seq 2's hash, seq 6's hash and the head")
		}
	})
	t.Run("an empty window reports no ends and no hashes, but still the head", func(t *testing.T) {
		m := manifestOf(t, ctx, st, org, minuteAt(8).Add(time.Second), time.Time{})
		if m.FirstSeq != 0 || m.LastSeq != 0 || m.Count != 0 || m.HeadSeq != 8 {
			t.Fatalf("empty-window manifest = %+v", m)
		}
		if len(m.FirstPrevHash) != 0 || len(m.LastRowHash) != 0 || !bytes.Equal(m.HeadHash, rows[7].hash) {
			t.Fatal("empty-window manifest carries range hashes")
		}
	})
	t.Run("a test signer's signature verifies over ManifestPayload for every range", func(t *testing.T) {
		signer := newManifestTestSigner(1)
		signed := store.New(pool, store.WithChain(), store.WithChainKeys(ring), store.WithManifestSigner(signer))
		windows := []struct {
			name         string
			since, until time.Time
		}{
			{"whole chain", time.Time{}, time.Time{}},
			{"window", minuteAt(3), minuteAt(6)},
			{"empty window", minuteAt(8).Add(time.Second), time.Time{}},
		}
		for _, w := range windows {
			m := manifestOf(t, ctx, signed, org, w.since, w.until)
			if m.KeyID != "audit-test-a" {
				t.Fatalf("%s: key id %q, want the signer's", w.name, m.KeyID)
			}
			payload, err := store.ManifestPayload(m)
			if err != nil {
				t.Fatal(err)
			}
			if !ed25519.Verify(signer.public(), payload, m.Signature) {
				t.Fatalf("%s: signature does not verify over ManifestPayload", w.name)
			}
			m.HeadSeq++
			tampered, err := store.ManifestPayload(m)
			if err != nil {
				t.Fatal(err)
			}
			if ed25519.Verify(signer.public(), tampered, m.Signature) {
				t.Fatalf("%s: a changed head still verifies", w.name)
			}
		}
	})
	t.Run("without a signer the key ring signs with its active key", func(t *testing.T) {
		m := manifestOf(t, ctx, st, org, time.Time{}, time.Time{})
		if m.KeyID != "audit-test-a" || len(m.Signature) != ed25519.SignatureSize {
			t.Fatalf("manifest key %q with %d signature bytes, want the ring's active key", m.KeyID, len(m.Signature))
		}
		payload, err := store.ManifestPayload(m)
		if err != nil {
			t.Fatal(err)
		}
		if err := ring.VerifyPayload(m.KeyID, payload, m.Signature); err != nil {
			t.Fatalf("the ring does not verify its own manifest: %v", err)
		}
	})
	t.Run("a store with no key serves the manifest unsigned without failing", func(t *testing.T) {
		m := manifestOf(t, ctx, store.New(pool, store.WithChain()), org, time.Time{}, time.Time{})
		if m.KeyID != "" || m.Signature != nil || m.Count != 8 {
			t.Fatalf("keyless manifest key %q signature %x count %d, want unsigned with the range", m.KeyID, m.Signature, m.Count)
		}
	})
	t.Run("a verify-only ring serves the manifest unsigned", func(t *testing.T) {
		verifyOnly, err := chain.NewKeyRing(chain.KeyConfig{KeyID: "audit-test-a", Public: publicOf(1)})
		if err != nil {
			t.Fatal(err)
		}
		m := manifestOf(t, ctx, store.New(pool, store.WithChain(), store.WithChainKeys(verifyOnly)), org, time.Time{}, time.Time{})
		if m.KeyID != "" || m.Signature != nil {
			t.Fatalf("verify-only manifest key %q signature %x, want unsigned", m.KeyID, m.Signature)
		}
	})
	t.Run("an org that was never chained reports an empty head", func(t *testing.T) {
		unchained := pruneOrg(t, pool, "db-manifest-none")
		if _, err := store.New(pool).InsertAudit(ctx, chainEvent(unchained, 1)); err != nil {
			t.Fatalf("insert unchained row: %v", err)
		}
		m := manifestOf(t, ctx, st, unchained, time.Time{}, time.Time{})
		if m.LegacyUnchainedRows != 1 || m.HeadSeq != 0 || m.Count != 0 || m.FirstSeq != 0 || m.LastSeq != 0 {
			t.Fatalf("never-chained manifest = %+v", m)
		}
		if len(m.HeadHash) != 0 || len(m.FirstPrevHash) != 0 || len(m.LastRowHash) != 0 {
			t.Fatal("never-chained manifest carries chain hashes")
		}
	})
}

// TestComplianceDBKeysForOneOrg attests two orgs under two keys. Org B is
// attested under the old key, then the ring rotates and org A is attested under
// the new key. Each org's listing must name only the keys its own newest anchors
// used, so org A never shows org B's retired key.
func TestComplianceDBKeysForOneOrg(t *testing.T) {
	ctx, pool := chainDB(t)
	oldRing := pruneRing(t, "audit-old", 3, "")
	newRing := pruneRing(t, "audit-new", 4, "audit-old:"+publicOf(3))

	orgB := pruneOrg(t, pool, "db-keys-b")
	oldSt := store.New(pool, store.WithChain(), store.WithChainKeys(oldRing))
	insertChained(t, ctx, oldSt, orgB, 1, 3)
	if _, err := oldSt.Attest(ctx); err != nil {
		t.Fatalf("attest under the old key: %v", err)
	}

	orgA := pruneOrg(t, pool, "db-keys-a")
	newSt := store.New(pool, store.WithChain(), store.WithChainKeys(newRing))
	insertChained(t, ctx, newSt, orgA, 1, 2)
	if _, err := newSt.Attest(ctx); err != nil {
		t.Fatalf("attest under the new key: %v", err)
	}
	// Created after the attest, so this org has a chain and no attestation.
	orgC := pruneOrg(t, pool, "db-keys-c")
	insertChained(t, ctx, newSt, orgC, 1, 1)

	active := store.ComplianceKey{KeyID: "audit-new", Algorithm: "ed25519", PublicKey: publicOf(4), Status: store.KeyStatusActive}
	retired := store.ComplianceKey{KeyID: "audit-old", Algorithm: "ed25519", PublicKey: publicOf(3), Status: store.KeyStatusRetired}

	t.Run("an org attested under the active key lists only that key", func(t *testing.T) {
		keys, err := newSt.ComplianceKeys(ctx, orgA)
		if err != nil {
			t.Fatal(err)
		}
		requireKeys(t, keys, active)
	})
	t.Run("an org attested under a retired key lists it beside the active key", func(t *testing.T) {
		keys, err := newSt.ComplianceKeys(ctx, orgB)
		if err != nil {
			t.Fatal(err)
		}
		requireKeys(t, keys, active, retired)
	})
	t.Run("an org never attested lists only the active key", func(t *testing.T) {
		keys, err := newSt.ComplianceKeys(ctx, orgC)
		if err != nil {
			t.Fatal(err)
		}
		requireKeys(t, keys, active)
	})
	t.Run("a server without the key lists the retired key as unavailable", func(t *testing.T) {
		keys, err := store.New(pool, store.WithChain()).ComplianceKeys(ctx, orgB)
		if err != nil {
			t.Fatal(err)
		}
		requireKeys(t, keys, store.ComplianceKey{KeyID: "audit-old", Algorithm: "ed25519", Status: store.KeyStatusUnavailable})
	})
}
