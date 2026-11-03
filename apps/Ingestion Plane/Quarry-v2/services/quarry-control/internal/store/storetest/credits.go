package storetest

import (
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// CreditUsageConformance runs the team credit rule (INNH-9) against any
// TeamUsageStore: only page_fetched events earn credits, each at
// store.CreditsPerPage, scoped to the job's org. Other events of the same job
// (run_started, page_queued, page_failed, artifact_written) earn nothing, so the
// team figure equals the sum of the per-job credits on the job reads. The
// Postgres store used to count every event with a job id as one credit.
func CreditUsageConformance(t *testing.T, newDB func(t *testing.T) store.DB) {
	t.Helper()
	db := newDB(t)

	seedJob := func(org string) quarrycontracts.ID {
		id := quarrycontracts.NewID(quarrycontracts.KindJob)
		if err := db.Jobs().Create(store.Job{
			ID: id, OrgID: org, Kind: "crawl", Status: "running",
			Params:    map[string]any{},
			Origin:    store.JobOrigin{Type: "user", Actor: "usr_1"},
			CreatedAt: time.Now().UnixMilli(),
		}); err != nil {
			t.Fatalf("seed job: %v", err)
		}
		return id
	}
	emit := func(job quarrycontracts.ID, typ quarrycontracts.EventType, n int) {
		for i := 0; i < n; i++ {
			jid := job
			if err := db.Events().Append(quarrycontracts.Event{
				EventID:   quarrycontracts.NewID(quarrycontracts.KindEvent),
				JobID:     &jid,
				Type:      typ,
				Timestamp: time.Now().UTC(),
				Payload:   map[string]any{"url": "https://example.com/p"},
			}); err != nil {
				t.Fatalf("append %s: %v", typ, err)
			}
		}
	}

	jobA := seedJob("org_credit_a")
	emit(jobA, quarrycontracts.EvtRunStarted, 1)
	emit(jobA, quarrycontracts.EvtPageQueued, 4)
	emit(jobA, quarrycontracts.EvtPageFetched, 3)
	emit(jobA, quarrycontracts.EvtPageFailed, 1)
	jobA2 := seedJob("org_credit_a")
	emit(jobA2, quarrycontracts.EvtPageFetched, 2)
	jobB := seedJob("org_credit_b")
	emit(jobB, quarrycontracts.EvtPageFetched, 7)

	t.Run("only page_fetched events earn credits", func(t *testing.T) {
		row, err := db.TeamUsage().CreditUsage("org_credit_a", "7d")
		if err != nil {
			t.Fatal(err)
		}
		if want := 5 * store.CreditsPerPage; row.CreditsUsed != want {
			t.Fatalf("org a credits_used=%v want %v", row.CreditsUsed, want)
		}
	})
	t.Run("another org's events are not summed", func(t *testing.T) {
		row, err := db.TeamUsage().CreditUsage("org_credit_b", "7d")
		if err != nil {
			t.Fatal(err)
		}
		if want := 7 * store.CreditsPerPage; row.CreditsUsed != want {
			t.Fatalf("org b credits_used=%v want %v", row.CreditsUsed, want)
		}
	})
	t.Run("an org with no jobs has zero credits", func(t *testing.T) {
		row, err := db.TeamUsage().CreditUsage("org_credit_none", "7d")
		if err != nil {
			t.Fatal(err)
		}
		if row.CreditsUsed != 0 {
			t.Fatalf("credits_used=%v want 0", row.CreditsUsed)
		}
	})
}
