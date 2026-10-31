package jobs

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/mocks"
)

// control is a stand-in for quarry-control: it serves the claim endpoint (or
// answers 404 for it), the list endpoint and the PUT that marks a job running,
// and records what the dispatcher asked.
type control struct {
	mu        sync.Mutex
	claim     func() (int, any) // status and body for POST /v1/internal/jobs/claim
	list      []map[string]any
	claimBody []map[string]int
	listed    int
	puts      []map[string]string
	putStatus int
}

func (c *control) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/jobs/claim", func(w http.ResponseWriter, r *http.Request) {
		c.mu.Lock()
		defer c.mu.Unlock()
		var body map[string]int
		_ = json.NewDecoder(r.Body).Decode(&body)
		c.claimBody = append(c.claimBody, body)
		status, payload := c.claim()
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(status)
		_ = json.NewEncoder(w).Encode(payload)
	})
	mux.HandleFunc("/v1/jobs/", func(w http.ResponseWriter, r *http.Request) {
		c.mu.Lock()
		defer c.mu.Unlock()
		if r.Method == http.MethodPut {
			raw, _ := io.ReadAll(r.Body)
			var patch map[string]string
			_ = json.Unmarshal(raw, &patch)
			c.puts = append(c.puts, patch)
			status := c.putStatus
			if status == 0 {
				status = http.StatusOK
			}
			w.WriteHeader(status)
			_, _ = w.Write([]byte("{}"))
			return
		}
		c.listed++
		_ = json.NewEncoder(w).Encode(c.list)
	})
	return mux
}

func crawlJob(id, org string) map[string]any {
	return map[string]any{
		"id": id, "kind": "crawl", "status": "accepted", "created_at": "2026-10-16T10:00:00Z",
		"params": map[string]any{"url": "https://example.com", "org_id": org},
	}
}

func newManager(t *testing.T, ctl *control, batch int) (*Manager, *mocks.Client, *int) {
	t.Helper()
	server := httptest.NewServer(ctl.handler())
	t.Cleanup(server.Close)
	temporal := &mocks.Client{}
	started := 0
	run := &mocks.WorkflowRun{}
	run.On("GetID").Return("quarry-job-x")
	run.On("GetRunID").Return("temporal-run")
	temporal.On("ExecuteWorkflow", mock.Anything, mock.Anything, "CrawlJobWF", mock.Anything).
		Run(func(mock.Arguments) { started++ }).
		Return(client.WorkflowRun(run), nil)
	return New(temporal, Config{ControlBaseURL: server.URL, TaskQueue: "q", ClaimBatch: batch}), temporal, &started
}

func TestDispatcherUsesClaimWhenControlServesIt(t *testing.T) {
	ctl := &control{claim: func() (int, any) {
		return http.StatusOK, map[string]any{"data": []map[string]any{crawlJob("job_1", "org_a")}}
	}}
	manager, _, started := newManager(t, ctl, 7)
	manager.tick(context.Background())

	require.Equal(t, 1, *started, "the claimed job is started")
	require.Equal(t, 0, ctl.listed, "no list read while claiming works: that window is what admission replaces")
	require.Equal(t, []map[string]int{{"limit": 7}}, ctl.claimBody)
	require.Len(t, ctl.puts, 1)
	require.Equal(t, "running", ctl.puts[0]["status"])
}

func TestDispatcherFallsBackToTheListWhenClaimIsNotServed(t *testing.T) {
	ctl := &control{
		claim: func() (int, any) { return http.StatusNotFound, map[string]any{"error": "not enabled"} },
		list:  []map[string]any{crawlJob("job_1", "org_a")},
	}
	manager, _, started := newManager(t, ctl, 0)
	manager.tick(context.Background())

	require.Equal(t, 1, *started)
	require.Equal(t, 1, ctl.listed, "a 404 on claim means list, as before admission existed")
	require.Equal(t, defaultClaimBatch, ctl.claimBody[0]["limit"], "an unset batch is the default")
}

func TestDispatcherAcceptsABareArrayClaimResponse(t *testing.T) {
	ctl := &control{claim: func() (int, any) {
		return http.StatusOK, []map[string]any{crawlJob("job_1", "org_a")}
	}}
	manager, _, started := newManager(t, ctl, 5)
	manager.tick(context.Background())
	require.Equal(t, 1, *started)
}

func TestAClaimErrorIsNotAFallbackAndStartsNothing(t *testing.T) {
	ctl := &control{
		claim: func() (int, any) { return http.StatusInternalServerError, map[string]any{"error": "db down"} },
		list:  []map[string]any{crawlJob("job_1", "org_a")},
	}
	manager, _, started := newManager(t, ctl, 5)
	manager.tick(context.Background())
	require.Equal(t, 0, *started, "falling back to an uncapped list on a server error would defeat the caps")
	require.Equal(t, 0, ctl.listed)
}

func TestAClaimedJobWhoseMarkRunningWasLostIsMarkedWhenItComesBack(t *testing.T) {
	ctl := &control{putStatus: http.StatusInternalServerError, claim: func() (int, any) {
		return http.StatusOK, map[string]any{"data": []map[string]any{crawlJob("job_1", "org_a")}}
	}}
	manager, _, started := newManager(t, ctl, 5)

	manager.tick(context.Background()) // starts the workflow; the write that marks it running fails
	require.Equal(t, 1, *started)
	require.Len(t, ctl.puts, 1)

	ctl.putStatus = http.StatusOK
	manager.tick(context.Background()) // control hands the same accepted job back after its lease
	require.Equal(t, 1, *started, "the workflow is not started twice")
	require.Len(t, ctl.puts, 2, "but the lost 'running' mark is written again")
	require.Equal(t, "running", ctl.puts[1]["status"])
	require.Equal(t, ctl.puts[0]["run_id"], ctl.puts[1]["run_id"], "with the run that was started")
}

func TestLoadClaimBatch(t *testing.T) {
	t.Setenv("QUARRY_JOB_CLAIM_BATCH", "")
	n, err := LoadClaimBatch()
	require.NoError(t, err)
	require.Equal(t, defaultClaimBatch, n)

	t.Setenv("QUARRY_JOB_CLAIM_BATCH", "20")
	n, err = LoadClaimBatch()
	require.NoError(t, err)
	require.Equal(t, 20, n)

	for _, bad := range []string{"0", "-1", "lots", "501"} {
		t.Setenv("QUARRY_JOB_CLAIM_BATCH", bad)
		_, err = LoadClaimBatch()
		require.ErrorContains(t, err, "QUARRY_JOB_CLAIM_BATCH", bad)
	}
}
