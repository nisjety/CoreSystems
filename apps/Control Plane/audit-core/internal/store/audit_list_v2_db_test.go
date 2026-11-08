package store_test

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// WS05-S4: store.ListAudit keyset paging and filters. These need Postgres
// (TEST_DATABASE_URL, like the other chain tests) and skip without it.

// listWindow is wide enough to hold the restamped instants of these tests.
func listWindow() (time.Time, time.Time) {
	return dbBase.Add(-time.Hour), dbBase.Add(24 * time.Hour)
}

func listAll(t *testing.T, ctx context.Context, st *store.Store, f store.AuditFilter) []store.AuditRow {
	t.Helper()
	f.Since, f.Until = listWindow()
	rows, err := st.ListAudit(ctx, f)
	if err != nil {
		t.Fatalf("ListAudit: %v", err)
	}
	return rows
}

func eventSeq(t *testing.T, org string, row store.AuditRow) int {
	t.Helper()
	var n int
	if _, err := fmt.Sscanf(row.EventID, "chain:"+org+":%d", &n); err != nil {
		t.Fatalf("event id %q is not a seeded one: %v", row.EventID, err)
	}
	return n
}

func TestListAuditDBOrderCursorAndTiesPageWithoutLossOrDuplicate(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "list-v2-ties")
	st := store.New(pool)
	insertChained(t, ctx, st, org, 1, 6)
	// Pairs (1,2), (3,4), (5,6) share an ingest instant: only the id orders them.
	restampIngestedAt(t, ctx, pool, org, func(seq int64) time.Time { return minuteAt(int((seq + 1) / 2)) })

	all := listAll(t, ctx, st, store.AuditFilter{OrgID: org, Limit: 50})
	if len(all) != 6 {
		t.Fatalf("got %d rows, want 6", len(all))
	}
	// (ingested_at DESC, id DESC): newest pair first, higher id first within a pair.
	for i, row := range all {
		if want := 6 - i; eventSeq(t, org, row) != want {
			t.Fatalf("row %d is seq %d, want %d", i, eventSeq(t, org, row), want)
		}
	}
	if !all[0].IngestedAt.Equal(all[1].IngestedAt) || all[0].ID <= all[1].ID {
		t.Fatalf("seq 6 and 5 share an instant and order by id DESC: %+v %+v", all[0], all[1])
	}

	// Page one row at a time through the tie groups.
	var paged []store.AuditRow
	f := store.AuditFilter{OrgID: org, Limit: 1}
	for i := 0; i < 10; i++ {
		page := listAll(t, ctx, st, f)
		if len(page) == 0 {
			break
		}
		paged = append(paged, page...)
		last := page[len(page)-1]
		f.HasCursor, f.CursorIngestedAt, f.CursorID = true, last.IngestedAt, last.ID
	}
	if len(paged) != len(all) {
		t.Fatalf("paged %d rows, want %d", len(paged), len(all))
	}
	for i := range all {
		if paged[i].ID != all[i].ID {
			t.Fatalf("page walk differs at %d: %d vs %d", i, paged[i].ID, all[i].ID)
		}
	}

	// The cursor row itself is excluded, and a cursor in the middle of a tie
	// returns only the lower id of that tie plus everything older.
	mid := all[2] // seq 4: ties with seq 3
	after := listAll(t, ctx, st, store.AuditFilter{OrgID: org, Limit: 50,
		HasCursor: true, CursorIngestedAt: mid.IngestedAt, CursorID: mid.ID})
	if len(after) != 3 || eventSeq(t, org, after[0]) != 3 {
		t.Fatalf("after seq 4: %d rows", len(after))
	}

	// A cursor at the oldest row yields an empty page.
	oldest := all[len(all)-1]
	none := listAll(t, ctx, st, store.AuditFilter{OrgID: org, Limit: 5,
		HasCursor: true, CursorIngestedAt: oldest.IngestedAt, CursorID: oldest.ID})
	if len(none) != 0 {
		t.Fatalf("cursor at the oldest row must return nothing, got %d", len(none))
	}
}

func TestListAuditDBFiltersNarrowAndStayInTheOrg(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "list-v2-filters")
	other := pruneOrg(t, pool, "list-v2-filters-other")
	st := store.New(pool)

	type seed struct{ plane, outcome, subject, resource, user string }
	seeds := []seed{
		{"control", "ok", "sub-a", "res-1", "user-1"},
		{"model", "ok", "sub-a", "res-2", "user-2"},
		{"model", "denied", "sub-b", "res-2", "user-1"},
		{"frontend", "denied", "sub-b", "res-3", "user-2"},
	}
	for i, s := range seeds {
		ev := chainEvent(org, i+1)
		ev.Plane, ev.Outcome, ev.Subject, ev.ResourceID, ev.UserID = s.plane, s.outcome, s.subject, s.resource, s.user
		if _, err := st.InsertAudit(ctx, ev); err != nil {
			t.Fatal(err)
		}
	}
	foreign := chainEvent(other, 1)
	foreign.Plane = "model"
	foreign.Outcome = "denied"
	if _, err := st.InsertAudit(ctx, foreign); err != nil {
		t.Fatal(err)
	}
	restampIngestedAt(t, ctx, pool, org, func(seq int64) time.Time { return minuteAt(int(seq)) })
	restampIngestedAt(t, ctx, pool, other, func(seq int64) time.Time { return minuteAt(int(seq)) })

	cases := []struct {
		name string
		f    store.AuditFilter
		want []int // seqs, newest first
	}{
		{"none", store.AuditFilter{}, []int{4, 3, 2, 1}},
		{"plane", store.AuditFilter{Plane: "model"}, []int{3, 2}},
		{"outcome", store.AuditFilter{Outcome: "denied"}, []int{4, 3}},
		{"subject", store.AuditFilter{Subject: "sub-a"}, []int{2, 1}},
		{"resource", store.AuditFilter{ResourceID: "res-2"}, []int{3, 2}},
		{"user", store.AuditFilter{UserID: "user-1"}, []int{3, 1}},
		{"plane+outcome", store.AuditFilter{Plane: "model", Outcome: "denied"}, []int{3}},
		{"all five", store.AuditFilter{Plane: "model", Outcome: "denied", Subject: "sub-b", ResourceID: "res-2", UserID: "user-1"}, []int{3}},
		{"no match", store.AuditFilter{Plane: "control", Outcome: "denied"}, nil},
	}
	for _, tc := range cases {
		tc.f.OrgID, tc.f.Limit = org, 50
		rows := listAll(t, ctx, st, tc.f)
		if len(rows) != len(tc.want) {
			t.Fatalf("%s: got %d rows, want %d", tc.name, len(rows), len(tc.want))
		}
		for i, row := range rows {
			if row.OrgID != org || eventSeq(t, org, row) != tc.want[i] {
				t.Fatalf("%s: row %d is %+v, want seq %d", tc.name, i, row, tc.want[i])
			}
		}
	}
}

func TestListAuditDBLimitUpToFiveHundredAndOneIsHonoured(t *testing.T) {
	ctx, pool := chainDB(t)
	org := pruneOrg(t, pool, "list-v2-limit")
	st := store.New(pool)
	const total = 105
	insertChained(t, ctx, st, org, 1, total)

	rows, err := st.ListAudit(ctx, store.AuditFilter{OrgID: org, Limit: store.MaxAuditListLimit + 1})
	if err != nil || len(rows) != total {
		t.Fatalf("Limit %d: got %d rows, err %v, want all %d", store.MaxAuditListLimit+1, len(rows), err, total)
	}
	rows, err = st.ListAudit(ctx, store.AuditFilter{OrgID: org, Limit: 7})
	if err != nil || len(rows) != 7 {
		t.Fatalf("Limit 7: got %d rows, err %v", len(rows), err)
	}
	// Out-of-range limits fall back to the 100-row default, never unbounded.
	for _, limit := range []int{0, -1, store.MaxAuditListLimit + 2, 100000} {
		rows, err = st.ListAudit(ctx, store.AuditFilter{OrgID: org, Limit: limit})
		if err != nil || len(rows) != 100 {
			t.Fatalf("Limit %d: got %d rows, err %v, want the 100-row default", limit, len(rows), err)
		}
	}
}

// Migrate is safe to run twice, and 011 re-applies cleanly once its ledger row
// is gone (the SQL is IF NOT EXISTS).
func TestMigrate011ReappliesCleanlyAfterLedgerRowIsRemoved(t *testing.T) {
	ctx, pool := chainDB(t)
	if err := store.Migrate(ctx, pool); err != nil {
		t.Fatalf("second Migrate: %v", err)
	}
	var recorded int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM schema_migrations WHERE version = '011_audit_read_indexes'`).Scan(&recorded); err != nil || recorded != 1 {
		t.Fatalf("011 ledger rows = %d err %v, want exactly 1", recorded, err)
	}
	if _, err := pool.Exec(ctx, `DELETE FROM schema_migrations WHERE version = '011_audit_read_indexes'`); err != nil {
		t.Fatal(err)
	}
	if err := store.Migrate(ctx, pool); err != nil {
		t.Fatalf("re-apply after ledger delete: %v", err)
	}
	var indexes int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM pg_indexes WHERE tablename = 'audit_events' AND indexname IN
		('idx_audit_events_org_ts_id','idx_audit_events_org_user_ts','idx_audit_events_org_plane_ts')`).Scan(&indexes); err != nil || indexes != 3 {
		t.Fatalf("read indexes present = %d err %v, want 3", indexes, err)
	}
}
