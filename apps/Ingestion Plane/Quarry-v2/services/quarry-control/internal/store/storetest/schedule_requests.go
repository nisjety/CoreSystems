package storetest

import (
	"errors"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// ScheduleRequestsConformance runs the request-table contract (WS15.S3) against
// any DB that implements store.ScheduleRequestsProvider, so the in-memory store
// and the Postgres store claim, lease and finish rows the same way. newDB must
// return an empty database per call (Claim is not org-scoped).
func ScheduleRequestsConformance(t *testing.T, newDB func(t *testing.T) store.DB) {
	t.Helper()
	now := time.Date(2026, 11, 3, 12, 0, 0, 0, time.UTC)

	reqsOf := func(t *testing.T, db store.DB) store.ScheduleRequestsStore {
		t.Helper()
		p, ok := db.(store.ScheduleRequestsProvider)
		if !ok {
			t.Fatal("DB does not implement store.ScheduleRequestsProvider")
		}
		return p.ScheduleRequests()
	}
	add := func(t *testing.T, s store.ScheduleRequestsStore, org string, sched quarrycontracts.ID, action string, created int64) quarrycontracts.ID {
		t.Helper()
		id := quarrycontracts.NewID(quarrycontracts.KindRequest)
		var params map[string]any
		if action == store.ScheduleRequestBackfill {
			params = map[string]any{"start_at": "2026-11-01T00:00:00Z", "end_at": "2026-11-02T00:00:00Z"}
		}
		if err := s.Create(store.ScheduleRequest{
			ID: id, ScheduleID: sched, OrgID: org, Action: action, Params: params,
			Status: store.ScheduleRequestPending, CreatedAt: created,
		}); err != nil {
			t.Fatalf("create: %v", err)
		}
		return id
	}
	newSched := func() quarrycontracts.ID { return quarrycontracts.NewID(quarrycontracts.KindSchedule) }

	t.Run("CreateThenGetByOrgIsTenantScoped", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		sched := newSched()
		id := add(t, s, "org_a", sched, store.ScheduleRequestBackfill, 100)
		got, ok := s.GetByOrg(id, "org_a")
		if !ok {
			t.Fatal("own org cannot read its request")
		}
		if got.Status != store.ScheduleRequestPending || got.Action != store.ScheduleRequestBackfill || got.ScheduleID != sched {
			t.Fatalf("round trip: %+v", got)
		}
		if got.Params["start_at"] != "2026-11-01T00:00:00Z" {
			t.Fatalf("params lost: %+v", got.Params)
		}
		if _, ok := s.GetByOrg(id, "org_b"); ok {
			t.Fatal("another org read the request")
		}
		if _, ok := s.GetByOrg(id, ""); ok {
			t.Fatal("an empty org read the request")
		}
	})

	t.Run("SecondOpenRequestOfOneScheduleConflicts", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		sched := newSched()
		add(t, s, "org_a", sched, store.ScheduleRequestTrigger, 100)
		err := s.Create(store.ScheduleRequest{
			ID: quarrycontracts.NewID(quarrycontracts.KindRequest), ScheduleID: sched, OrgID: "org_a",
			Action: store.ScheduleRequestBackfill, Status: store.ScheduleRequestPending, CreatedAt: 101,
		})
		if !errors.Is(err, store.ErrConflict) {
			t.Fatalf("second open request: err=%v, want ErrConflict", err)
		}
		// Another schedule is not affected.
		add(t, s, "org_a", newSched(), store.ScheduleRequestTrigger, 102)
	})

	t.Run("FinishedRequestFreesTheSchedule", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		sched := newSched()
		id := add(t, s, "org_a", sched, store.ScheduleRequestTrigger, 100)
		if _, err := s.Claim("w1", 10, time.Minute, now); err != nil {
			t.Fatal(err)
		}
		if err := s.Complete(id, "w1", map[string]any{"workflow_id": "wf-1"}, now); err != nil {
			t.Fatalf("complete: %v", err)
		}
		if s.HasPending(sched, "") {
			t.Fatal("a done request still counts as pending")
		}
		add(t, s, "org_a", sched, store.ScheduleRequestTrigger, 200)
		got, _ := s.GetByOrg(id, "org_a")
		if got.Status != store.ScheduleRequestDone || got.Result["workflow_id"] != "wf-1" || got.FinishedAt == nil {
			t.Fatalf("completed row: %+v", got)
		}
	})

	t.Run("DuplicateIdempotencyKeyConflicts", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		mk := func(sched quarrycontracts.ID, org string) error {
			return s.Create(store.ScheduleRequest{
				ID: quarrycontracts.NewID(quarrycontracts.KindRequest), ScheduleID: sched, OrgID: org,
				Action: store.ScheduleRequestTrigger, Status: store.ScheduleRequestPending,
				IdempotencyKey: "k1", CreatedAt: 100,
			})
		}
		if err := mk(newSched(), "org_a"); err != nil {
			t.Fatal(err)
		}
		if err := mk(newSched(), "org_a"); !errors.Is(err, store.ErrConflict) {
			t.Fatalf("same key same org: err=%v, want ErrConflict", err)
		}
		if err := mk(newSched(), "org_b"); err != nil {
			t.Fatalf("same key other org must be allowed: %v", err)
		}
	})

	t.Run("HasPendingFiltersByAction", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		sched := newSched()
		add(t, s, "org_a", sched, store.ScheduleRequestTrigger, 100)
		if !s.HasPending(sched, "") || !s.HasPending(sched, store.ScheduleRequestTrigger) {
			t.Fatal("open trigger not reported")
		}
		if s.HasPending(sched, store.ScheduleRequestBackfill) {
			t.Fatal("a trigger was reported as a backfill")
		}
		if s.HasPending(newSched(), "") {
			t.Fatal("an unknown schedule has a pending request")
		}
	})

	t.Run("ClaimGivesEachRowOnceOldestFirst", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		var ids []quarrycontracts.ID
		for i := 0; i < 3; i++ {
			ids = append(ids, add(t, s, "org_a", newSched(), store.ScheduleRequestTrigger, int64(100+i)))
		}
		first, err := s.Claim("w1", 2, time.Minute, now)
		if err != nil {
			t.Fatal(err)
		}
		if len(first) != 2 || first[0].ID != ids[0] || first[1].ID != ids[1] {
			t.Fatalf("first claim: %+v", first)
		}
		for _, c := range first {
			if c.Status != store.ScheduleRequestClaimed || c.ClaimedBy != "w1" || c.Attempts != 1 {
				t.Fatalf("claimed row: %+v", c)
			}
			if c.LeaseUntil != now.Add(time.Minute).UnixMilli() {
				t.Fatalf("lease_until %d", c.LeaseUntil)
			}
		}
		second, err := s.Claim("w2", 10, time.Minute, now)
		if err != nil {
			t.Fatal(err)
		}
		if len(second) != 1 || second[0].ID != ids[2] {
			t.Fatalf("second claim must get only the third row: %+v", second)
		}
		third, err := s.Claim("w3", 10, time.Minute, now)
		if err != nil {
			t.Fatal(err)
		}
		if len(third) != 0 {
			t.Fatalf("nothing left to claim, got %+v", third)
		}
	})

	t.Run("ConcurrentClaimsAreDisjoint", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		const n = 12
		for i := 0; i < n; i++ {
			add(t, s, "org_a", newSched(), store.ScheduleRequestTrigger, int64(100+i))
		}
		var mu sync.Mutex
		seen := map[quarrycontracts.ID]int{}
		var wg sync.WaitGroup
		for w := 0; w < 4; w++ {
			wg.Add(1)
			go func(worker string) {
				defer wg.Done()
				for {
					got, err := s.Claim(worker, 2, time.Minute, now)
					if err != nil {
						t.Errorf("claim: %v", err)
						return
					}
					if len(got) == 0 {
						return
					}
					mu.Lock()
					for _, g := range got {
						seen[g.ID]++
					}
					mu.Unlock()
				}
			}([]string{"wa", "wb", "wc", "wd"}[w])
		}
		wg.Wait()
		if len(seen) != n {
			t.Fatalf("claimed %d distinct rows, want %d", len(seen), n)
		}
		for id, c := range seen {
			if c != 1 {
				t.Fatalf("row %s claimed %d times", id, c)
			}
		}
	})

	t.Run("ExpiredLeaseIsReclaimed", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		id := add(t, s, "org_a", newSched(), store.ScheduleRequestTrigger, 100)
		if got, _ := s.Claim("w1", 1, 10*time.Second, now); len(got) != 1 {
			t.Fatalf("first claim: %+v", got)
		}
		if got, _ := s.Claim("w2", 1, 10*time.Second, now.Add(5*time.Second)); len(got) != 0 {
			t.Fatalf("a live lease was taken: %+v", got)
		}
		got, err := s.Claim("w2", 1, 10*time.Second, now.Add(11*time.Second))
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 1 || got[0].ID != id || got[0].ClaimedBy != "w2" || got[0].Attempts != 2 {
			t.Fatalf("reclaim: %+v", got)
		}
		// The first worker lost the row: its late result changes nothing.
		if err := s.Complete(id, "w1", nil, now.Add(12*time.Second)); !errors.Is(err, store.ErrConflict) {
			t.Fatalf("late complete by the old worker: err=%v, want ErrConflict", err)
		}
		if err := s.Complete(id, "w2", nil, now.Add(12*time.Second)); err != nil {
			t.Fatalf("complete by the holder: %v", err)
		}
	})

	t.Run("FailAndReleaseSemantics", func(t *testing.T) {
		s := reqsOf(t, newDB(t))
		sched := newSched()
		id := add(t, s, "org_a", sched, store.ScheduleRequestTrigger, 100)
		if err := s.Fail(id, "w1", store.ScheduleRequestFailed, "x", now); !errors.Is(err, store.ErrConflict) {
			t.Fatalf("fail of an unclaimed row: err=%v, want ErrConflict", err)
		}
		if err := s.Complete(quarrycontracts.NewID(quarrycontracts.KindRequest), "w1", nil, now); !errors.Is(err, store.ErrNotFound) {
			t.Fatalf("complete of an unknown row: err=%v, want ErrNotFound", err)
		}
		if _, err := s.Claim("w1", 1, time.Minute, now); err != nil {
			t.Fatal(err)
		}
		if err := s.Release(id, "w1", "temporal unavailable"); err != nil {
			t.Fatalf("release: %v", err)
		}
		got, _ := s.GetByOrg(id, "org_a")
		if got.Status != store.ScheduleRequestPending || got.ClaimedBy != "" || got.Attempts != 1 || got.Error != "temporal unavailable" {
			t.Fatalf("released row: %+v", got)
		}
		if !s.HasPending(sched, "") {
			t.Fatal("a released row must still block a second request")
		}
		again, _ := s.Claim("w2", 1, time.Minute, now)
		if len(again) != 1 || again[0].Attempts != 2 {
			t.Fatalf("reclaim after release: %+v", again)
		}
		if err := s.Fail(id, "w2", store.ScheduleRequestDone, "x", now); err == nil {
			t.Fatal("Fail accepted a non-failure status")
		}
		if err := s.Fail(id, "w2", store.ScheduleRequestRejected, "schedule not materialised", now); err != nil {
			t.Fatalf("reject: %v", err)
		}
		got, _ = s.GetByOrg(id, "org_a")
		if got.Status != store.ScheduleRequestRejected || got.Error != "schedule not materialised" || got.FinishedAt == nil {
			t.Fatalf("rejected row: %+v", got)
		}
		if s.HasPending(sched, "") {
			t.Fatal("a rejected row still blocks the schedule")
		}
		if got, _ := s.Claim("w3", 10, time.Minute, now.Add(time.Hour)); len(got) != 0 {
			t.Fatalf("a terminal row was claimed: %+v", got)
		}
	})
}
