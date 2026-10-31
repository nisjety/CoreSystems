// Package storetest holds contract tests that every implementation of a store
// interface must pass, so the in-memory and Postgres stores cannot drift.
package storetest

import (
	"fmt"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// AdmissionConformance runs the claim contract against any JobsStore, so the
// in-memory store and the Postgres store are held to the same behaviour.
func AdmissionConformance(t *testing.T, newDB func(t *testing.T) store.DB) {
	t.Helper()
	now := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	lease := time.Minute

	add := func(t *testing.T, db store.DB, org, kind, status string, created int64, params map[string]any) quarrycontracts.ID {
		t.Helper()
		id := quarrycontracts.NewID(quarrycontracts.KindJob)
		if err := db.Jobs().Create(store.Job{
			ID: id, OrgID: org, Kind: kind, Status: status, CreatedAt: created, Params: params,
		}); err != nil {
			t.Fatal(err)
		}
		return id
	}
	orgs := func(jobs []store.Job) []string {
		out := make([]string, 0, len(jobs))
		for _, j := range jobs {
			out = append(out, j.OrgID)
		}
		return out
	}
	// Two runs of the suite may share one database: tag organizations.
	tag := func(name string) string { return name + "_" + string(quarrycontracts.NewID(quarrycontracts.KindJob)) }

	t.Run("ClaimIsRoundRobinAcrossOrgs", func(t *testing.T) {
		db := newDB(t)
		a, b := tag("a"), tag("b")
		for i := 0; i < 4; i++ {
			add(t, db, a, "crawl", "accepted", int64(100+i), nil)
		}
		for i := 0; i < 2; i++ {
			add(t, db, b, "crawl", "accepted", int64(500+i), nil)
		}
		got, err := db.Jobs().ClaimAccepted(5, now, lease, 10)
		if err != nil {
			t.Fatal(err)
		}
		// Org a is older, but its second job waits for org b's first.
		want := []string{a, b, a, b, a}
		if fmt.Sprint(orgs(got)) != fmt.Sprint(want) {
			t.Fatalf("claimed orgs %v, want %v", orgs(got), want)
		}
	})

	t.Run("ClaimRespectsPerOrgCapIncludingLeasedAndRunningJobs", func(t *testing.T) {
		db := newDB(t)
		a := tag("a")
		running := add(t, db, a, "crawl", "running", 10, nil)
		for i := 0; i < 5; i++ {
			add(t, db, a, "crawl", "accepted", int64(100+i), nil)
		}
		// Cap 3, one already running: room for two.
		first, _ := db.Jobs().ClaimAccepted(10, now, lease, 3)
		if len(first) != 2 {
			t.Fatalf("claimed %d, want 2 (cap 3 minus 1 running)", len(first))
		}
		// Those two are leased and count against the cap: nothing more.
		if again, _ := db.Jobs().ClaimAccepted(10, now.Add(time.Second), lease, 3); len(again) != 0 {
			t.Fatalf("a leased job freed no room: claimed %d more", len(again))
		}
		// The running one finishes: still two leased, so room for exactly one.
		done, _ := db.Jobs().Get(running)
		done.Status = "completed"
		if err := db.Jobs().Update(done); err != nil {
			t.Fatal(err)
		}
		more, _ := db.Jobs().ClaimAccepted(10, now.Add(2*time.Second), lease, 3)
		if len(more) != 1 {
			t.Fatalf("claimed %d after a completion, want 1", len(more))
		}
	})

	t.Run("OldestAcceptedIsNotStarvedByNewerJobsOfAnotherOrg", func(t *testing.T) {
		db := newDB(t)
		a, b := tag("a"), tag("b")
		for i := 0; i < 60; i++ {
			add(t, db, a, "crawl", "accepted", int64(100+i), nil)
		}
		add(t, db, b, "crawl", "accepted", 9000, nil)
		got, _ := db.Jobs().ClaimAccepted(5, now, lease, 3)
		sawB := false
		for _, o := range orgs(got) {
			sawB = sawB || o == b
		}
		if !sawB {
			t.Fatalf("org b's only job was not in the first claim: %v", orgs(got))
		}
		if len(got) != 4 { // three of a (the cap) and one of b
			t.Fatalf("claimed %d, want 4", len(got))
		}
	})

	t.Run("ClaimLeaseExpiresAndTheJobIsReclaimable", func(t *testing.T) {
		db := newDB(t)
		a := tag("a")
		id := add(t, db, a, "crawl", "accepted", 100, nil)
		first, _ := db.Jobs().ClaimAccepted(1, now, lease, 3)
		if len(first) != 1 || first[0].ID != id {
			t.Fatalf("first claim: %#v", first)
		}
		if n := db.Jobs().CountAccepted(a, now.Add(time.Second)); n != 0 {
			t.Fatalf("a leased job is not waiting, but CountAccepted = %d", n)
		}
		if during, _ := db.Jobs().ClaimAccepted(1, now.Add(30*time.Second), lease, 3); len(during) != 0 {
			t.Fatal("claimed again before the lease ran out")
		}
		after, _ := db.Jobs().ClaimAccepted(1, now.Add(2*time.Minute), lease, 3)
		if len(after) != 1 || after[0].ID != id {
			t.Fatalf("an expired lease must be claimable again: %#v", after)
		}
		if n := db.Jobs().CountAccepted(a, now.Add(10*time.Minute)); n != 1 {
			t.Fatalf("after the second lease ran out the job is waiting again, CountAccepted = %d", n)
		}
	})

	t.Run("TheJobsOwnCapOverridesTheDefaultAndGarbageFallsBack", func(t *testing.T) {
		db := newDB(t)
		a, b, c := tag("a"), tag("b"), tag("c")
		for i := 0; i < 4; i++ {
			add(t, db, a, "crawl", "accepted", int64(100+i), map[string]any{"admission": map[string]any{"cap": float64(1)}})
			add(t, db, b, "crawl", "accepted", int64(100+i), map[string]any{"admission": map[string]any{"cap": "lots"}})
			add(t, db, c, "crawl", "accepted", int64(100+i), nil)
		}
		got, _ := db.Jobs().ClaimAccepted(50, now, lease, 2)
		perOrg := map[string]int{}
		for _, j := range got {
			perOrg[j.OrgID]++
		}
		if perOrg[a] != 1 || perOrg[b] != 2 || perOrg[c] != 2 {
			t.Fatalf("per-org claims = %v, want a:1 (own cap) b:2 (garbage falls back) c:2", perOrg)
		}
	})

	t.Run("OnlyKindsTheDispatcherExecuteAreClaimed", func(t *testing.T) {
		db := newDB(t)
		a := tag("a")
		add(t, db, a, "search", "accepted", 100, nil)
		add(t, db, a, "research", "accepted", 101, nil)
		keep := add(t, db, a, "scrape", "accepted", 102, nil)
		got, _ := db.Jobs().ClaimAccepted(10, now, lease, 5)
		if len(got) != 1 || got[0].ID != keep {
			t.Fatalf("claimed %#v", got)
		}
	})

	t.Run("FailExpiredFailsOnlyOldWaitingJobsAndRecordsWhy", func(t *testing.T) {
		db := newDB(t)
		a := tag("a")
		old := add(t, db, a, "crawl", "accepted", now.Add(-48*time.Hour).UnixMilli(), map[string]any{"url": "https://example.com"})
		fresh := add(t, db, a, "crawl", "accepted", now.Add(-time.Hour).UnixMilli(), nil)
		runningOld := add(t, db, a, "crawl", "running", now.Add(-72*time.Hour).UnixMilli(), nil)
		leasedOld := add(t, db, a, "crawl", "accepted", now.Add(-60*time.Hour).UnixMilli(), nil)
		// Lease the oldest waiting job: it is about to start and must not be failed.
		claimed, _ := db.Jobs().ClaimAccepted(1, now, lease, 10)
		if len(claimed) != 1 || claimed[0].ID != leasedOld {
			t.Fatalf("expected the oldest waiting job to be leased, got %#v", claimed)
		}
		failed, err := db.Jobs().FailExpired(now.Add(-24*time.Hour), now)
		if err != nil {
			t.Fatal(err)
		}
		ids := map[quarrycontracts.ID]store.Job{}
		for _, j := range failed {
			ids[j.ID] = j
		}
		for name, id := range map[string]quarrycontracts.ID{"fresh": fresh, "running": runningOld, "leased": leasedOld} {
			if _, ok := ids[id]; ok {
				t.Fatalf("the %s job was failed", name)
			}
		}
		got, ok := ids[old]
		if !ok || got.Status != "failed" || got.Params["failure_reason"] != "backlog_expired" || got.Params["url"] != "https://example.com" {
			t.Fatalf("expired job = %#v (listed %d)", got, len(failed))
		}
		stored, _ := db.Jobs().Get(old)
		if stored.Status != "failed" || stored.Params["failure_reason"] != "backlog_expired" {
			t.Fatalf("stored job = %#v", stored)
		}
	})
}

