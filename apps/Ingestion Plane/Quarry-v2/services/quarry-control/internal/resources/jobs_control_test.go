package resources

// Tests of the job control writes and the internal control reads (WS15.L2).
// Written in the build phase and compile-checked only; the wave's test phase runs
// them. The handlers run on the in-memory store through lcRouter (jobs_lifecycle_test.go).

import (
	"encoding/json"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-control/internal/store"
)

// ctlSeed stores a job with a workflow id, so the internal routes can see it.
func ctlSeed(t *testing.T, db store.DB, org, kind, status string) quarrycontracts.ID {
	t.Helper()
	id := lcSeedJob(t, db, org, kind, status, nil)
	j, ok := db.Jobs().GetByOrg(org, id)
	if !ok {
		t.Fatal("seeded job not found")
	}
	j.WorkflowID = "quarry-job-" + string(id)
	if err := db.Jobs().Update(j); err != nil {
		t.Fatal(err)
	}
	return id
}

// TestControl_CancelAcceptedJobIsImmediateAndDispatchCannotOverwrite: cancelling an
// accepted job ends it at once with run_cancelled, and the dispatcher's compare-and-
// set on accepted can no longer start it.
func TestControl_CancelAcceptedJobIsImmediateAndDispatchCannotOverwrite(t *testing.T) {
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "accepted", nil)

	rec := lcDo(h, http.MethodPost, "/v1/jobs/"+string(id)+"/control?org_id=org_a",
		map[string]any{"action": "cancel", "actor": "usr_1", "actor_type": "user"})
	if rec.Code != http.StatusOK {
		t.Fatalf("cancel accepted: status %d, body %s", rec.Code, rec.Body.String())
	}

	j, ok := db.Jobs().GetByOrg("org_a", id)
	if !ok {
		t.Fatal("job missing after cancel")
	}
	if j.Status != "cancelled" || j.TerminalReason != "run_cancelled" || j.FinishedAt == nil {
		t.Fatalf("cancelled accepted job: status=%q reason=%q finished=%v", j.Status, j.TerminalReason, j.FinishedAt)
	}

	put := lcDo(h, http.MethodPut, "/v1/jobs/"+string(id)+"?org_id=org_a",
		map[string]any{"status": "running", "run_id": "run_x", "if_status": "accepted"})
	if put.Code != http.StatusConflict {
		t.Fatalf("dispatcher CAS after cancel: status %d, want 409", put.Code)
	}
}

// TestControl_TerminalIs409: a request on a job that already ended is refused with
// the status and the reason in details.
func TestControl_TerminalIs409(t *testing.T) {
	h, db := lcRouter(t)
	id := lcSeedJob(t, db, "org_a", "crawl", "completed", nil)
	rec := lcDo(h, http.MethodPost, "/v1/jobs/"+string(id)+"/control?org_id=org_a",
		map[string]any{"action": "cancel"})
	if rec.Code != http.StatusConflict {
		t.Fatalf("cancel of a completed job: status %d, want 409", rec.Code)
	}
	if !strings.Contains(rec.Body.String(), "ended") {
		t.Fatalf("409 details should name the reason: %s", rec.Body.String())
	}
}

// TestControl_PauseWrongStateIs409: pause of a job that is not running, and resume
// of one that is not paused, are both 409.
func TestControl_PauseWrongStateIs409(t *testing.T) {
	h, db := lcRouter(t)
	accepted := lcSeedJob(t, db, "org_a", "crawl", "accepted", nil)
	rec := lcDo(h, http.MethodPost, "/v1/jobs/"+string(accepted)+"/control?org_id=org_a", map[string]any{"action": "pause"})
	if rec.Code != http.StatusConflict {
		t.Fatalf("pause of an accepted job: status %d, want 409", rec.Code)
	}

	running := ctlSeed(t, db, "org_a", "crawl", "running")
	rec = lcDo(h, http.MethodPost, "/v1/jobs/"+string(running)+"/control?org_id=org_a", map[string]any{"action": "resume"})
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "not_paused") {
		t.Fatalf("resume of a running job: status %d body %s", rec.Code, rec.Body.String())
	}
}

// TestControl_CrossOrgIs404: a job of another organization is a 404, never a
// conflict that would confirm its existence.
func TestControl_CrossOrgIs404(t *testing.T) {
	h, db := lcRouter(t)
	id := ctlSeed(t, db, "org_a", "crawl", "running")
	rec := lcDo(h, http.MethodPost, "/v1/jobs/"+string(id)+"/control?org_id=org_b", map[string]any{"action": "cancel"})
	if rec.Code != http.StatusNotFound {
		t.Fatalf("cross-org cancel: status %d, want 404", rec.Code)
	}
}

// TestControl_ScrapeNotControllable: a running scrape takes no pause, resume or
// cancel; only a cancel before it started is allowed.
func TestControl_ScrapeNotControllable(t *testing.T) {
	h, db := lcRouter(t)
	id := ctlSeed(t, db, "org_a", "scrape", "running")
	rec := lcDo(h, http.MethodPost, "/v1/jobs/"+string(id)+"/control?org_id=org_a", map[string]any{"action": "cancel"})
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "not_controllable") {
		t.Fatalf("cancel of a running scrape: status %d body %s", rec.Code, rec.Body.String())
	}
}

// TestControl_PauseRaisesDesiredSeqAndRepeatIsNoop: a pause on a running crawl sets
// desired_state and raises desired_seq; a repeat while it is pending changes nothing.
func TestControl_PauseRaisesDesiredSeqAndRepeatIsNoop(t *testing.T) {
	h, db := lcRouter(t)
	id := ctlSeed(t, db, "org_a", "crawl", "running")
	for i := 0; i < 2; i++ {
		rec := lcDo(h, http.MethodPost, "/v1/jobs/"+string(id)+"/control?org_id=org_a", map[string]any{"action": "pause"})
		if rec.Code != http.StatusOK {
			t.Fatalf("pause %d: status %d", i, rec.Code)
		}
	}
	j, _ := db.Jobs().GetByOrg("org_a", id)
	if j.DesiredState != "paused" || j.DesiredSeq != 1 {
		t.Fatalf("desired state=%q seq=%d, want paused and 1 (a repeat of a pending pause is a no-op)", j.DesiredState, j.DesiredSeq)
	}
}

// TestPendingControl_ReturnsOnlyUnapplied: the internal pending read lists the jobs
// whose desired state is ahead of what was applied, and drops them once acknowledged.
func TestPendingControl_ReturnsOnlyUnapplied(t *testing.T) {
	h, db := lcRouter(t)
	id := ctlSeed(t, db, "org_a", "crawl", "running")
	lcDo(h, http.MethodPost, "/v1/jobs/"+string(id)+"/control?org_id=org_a", map[string]any{"action": "pause"})

	rec := lcDo(h, http.MethodGet, "/v1/internal/jobs/pending-control", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("pending-control: status %d", rec.Code)
	}
	var pending []map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &pending); err != nil {
		t.Fatal(err)
	}
	if len(pending) != 1 {
		t.Fatalf("pending before ack: %d jobs, want 1", len(pending))
	}

	ack := lcDo(h, http.MethodPost, "/v1/internal/jobs/"+string(id)+"/control-ack", map[string]any{"applied_seq": 1})
	if ack.Code != http.StatusOK {
		t.Fatalf("control-ack: status %d", ack.Code)
	}

	rec = lcDo(h, http.MethodGet, "/v1/internal/jobs/pending-control", nil)
	pending = nil
	if err := json.Unmarshal(rec.Body.Bytes(), &pending); err != nil {
		t.Fatal(err)
	}
	if len(pending) != 0 {
		t.Fatalf("pending after ack: %d jobs, want 0", len(pending))
	}
}

// TestFinalize_IsIdempotent: finalizing an ended job changes nothing and writes no
// second event; the first finalize of a running job ends it once.
func TestFinalize_IsIdempotent(t *testing.T) {
	h, db := lcRouter(t)
	id := ctlSeed(t, db, "org_a", "crawl", "running")

	first := lcDo(h, http.MethodPost, "/v1/internal/jobs/"+string(id)+"/finalize",
		map[string]any{"status": "timed_out", "timeout_type": "execution", "limit_s": 20, "finished_at": time.Now().UnixMilli()})
	if first.Code != http.StatusOK || !strings.Contains(first.Body.String(), `"changed":true`) {
		t.Fatalf("first finalize: status %d body %s", first.Code, first.Body.String())
	}

	second := lcDo(h, http.MethodPost, "/v1/internal/jobs/"+string(id)+"/finalize",
		map[string]any{"status": "cancelled", "forced": true})
	if second.Code != http.StatusOK || !strings.Contains(second.Body.String(), `"changed":false`) {
		t.Fatalf("second finalize: status %d body %s", second.Code, second.Body.String())
	}

	j, _ := db.Jobs().Get(id)
	if j.Status != "timed_out" {
		t.Fatalf("status after a second finalize = %q, want timed_out", j.Status)
	}
}
