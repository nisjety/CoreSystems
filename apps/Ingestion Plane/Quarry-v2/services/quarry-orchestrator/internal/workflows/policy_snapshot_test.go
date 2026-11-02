package workflows

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// WS7.G 6b: the policy the edge resolved at the handoff rides on the job's params to every page.
// These tests cover the params -> input builders and the pages the workflows send. Adding the
// field changes no command of either workflow (it is only part of the page activity's input), so
// a history recorded before it replays unchanged; a job without a snapshot sends none.

const testSnapshot = `{"v":1,"modes":{"browser.act.input":"disabled"},"robots_max":"respect","proxy_max":"none","allow":[],"deny":["bad.example"]}`

func snapshotParams(t *testing.T) map[string]any {
	t.Helper()
	var snapshot map[string]any
	require.NoError(t, json.Unmarshal([]byte(testSnapshot), &snapshot))
	return map[string]any{
		"url": "https://snap.test/", "urls": []any{"https://snap.test/a", "https://snap.test/b"},
		"org_id": "org_snap", "policy_snapshot": snapshot,
	}
}

func TestInputsCarryThePolicySnapshotFromParams(t *testing.T) {
	t.Parallel()
	params := snapshotParams(t)

	crawl, err := CrawlInputFromParams(params, "run_1", "job_1", 2, 5)
	require.NoError(t, err)
	require.JSONEq(t, testSnapshot, string(crawl.PolicySnapshot))

	batch, err := BatchInputFromParams(params, "run_1", "job_1")
	require.NoError(t, err)
	require.JSONEq(t, testSnapshot, string(batch.PolicySnapshot))
}

func TestInputsWithoutAPolicySnapshotAreUnchanged(t *testing.T) {
	t.Parallel()
	for name, mutate := range map[string]func(map[string]any){
		"absent": func(p map[string]any) { delete(p, "policy_snapshot") },
		"null":   func(p map[string]any) { p["policy_snapshot"] = nil },
		"empty":  func(p map[string]any) { p["policy_snapshot"] = map[string]any{} },
		"wrong":  func(p map[string]any) { p["policy_snapshot"] = "not an object" },
	} {
		params := snapshotParams(t)
		mutate(params)
		crawl, err := CrawlInputFromParams(params, "run_1", "job_1", 2, 5)
		require.NoError(t, err, name)
		require.Nil(t, crawl.PolicySnapshot, name)
		batch, err := BatchInputFromParams(params, "run_1", "job_1")
		require.NoError(t, err, name)
		require.Nil(t, batch.PolicySnapshot, name)

		// The encoded input, which is what a workflow history records, has no key at all.
		encoded, err := json.Marshal(crawl)
		require.NoError(t, err)
		require.NotContains(t, string(encoded), "policy_snapshot", name)
	}
}

// A workflow input recorded before the field existed decodes with no snapshot.
func TestPreSnapshotInputsDecodeWithoutOne(t *testing.T) {
	t.Parallel()
	var crawl CrawlJobInput
	require.NoError(t, json.Unmarshal([]byte(`{"run_id":"r","seeds":["https://a.test/"],"max_depth":1,"max_pages":1}`), &crawl))
	require.Nil(t, crawl.PolicySnapshot)
	var batch BatchJobInput
	require.NoError(t, json.Unmarshal([]byte(`{"run_id":"r","urls":["https://a.test/"]}`), &batch))
	require.Nil(t, batch.PolicySnapshot)
}

func TestCrawlJobEveryPageCarriesThePolicySnapshot(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		in := scopeInput("run_snap_crawl")
		in.PolicySnapshot = json.RawMessage(testSnapshot)
		_, sent, _, err := scopeRun(t, durable, in, func(rp activities.RunPageInput) (activities.RunPageResult, error) {
			if rp.URL == "https://scope.test/" {
				return scopeApplied([]string{"https://scope.test/a"}, activities.CrawlReport{}), nil
			}
			return scopeApplied(nil, activities.CrawlReport{}), nil
		})
		require.NoError(t, err)
		require.Len(t, sent, 2)
		for _, page := range sent {
			require.JSONEq(t, testSnapshot, string(page.PolicySnapshot), "page %s", page.URL)
		}
	})
}

func TestCrawlJobWithoutASnapshotSendsNone(t *testing.T) {
	forEachFrontier(t, func(t *testing.T, durable bool) {
		_, sent, _, err := scopeRun(t, durable, scopeInput("run_snap_none"), func(activities.RunPageInput) (activities.RunPageResult, error) {
			return scopeApplied(nil, activities.CrawlReport{}), nil
		})
		require.NoError(t, err)
		require.NotEmpty(t, sent)
		for _, page := range sent {
			require.Nil(t, page.PolicySnapshot)
		}
	})
}

func TestBatchJobEveryPageCarriesThePolicySnapshot(t *testing.T) {
	env, a := newEnv(t)
	var sent []activities.RunPageInput
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			sent = append(sent, in)
			return activities.RunPageResult{Status: 200}, nil
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{
		RunID: "run_snap_batch", OrgID: "org_snap",
		URLs:           []string{"https://snap.test/a", "https://snap.test/b"},
		PolicySnapshot: json.RawMessage(testSnapshot),
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.Len(t, sent, 2)
	for _, page := range sent {
		require.JSONEq(t, testSnapshot, string(page.PolicySnapshot))
	}
}
