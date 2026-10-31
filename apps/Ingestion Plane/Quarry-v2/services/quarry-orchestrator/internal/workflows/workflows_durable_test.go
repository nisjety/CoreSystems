package workflows

import (
	"context"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// durableFakeQueue stands in for the Rust-owned PostgresRequestQueue behind the
// FrontierEnqueue, FrontierPop and FrontierAck activities. Like the real queue,
// an enqueue whose request id is already known is dropped, so a URL reached by
// two pages is fetched once. The fake is the test's own; it is not a copy of
// the Rust code.
type durableFakeQueue struct {
	pending  []activities.FrontierQueueItem
	known    map[string]struct{}
	enqueued int
	acked    map[string]int
}

func newDurableFakeQueue() *durableFakeQueue {
	return &durableFakeQueue{known: map[string]struct{}{}, acked: map[string]int{}}
}

func (q *durableFakeQueue) enqueue(_ context.Context, in activities.FrontierEnqueueInput) error {
	q.enqueued++
	if _, seen := q.known[in.RequestID]; seen {
		return nil
	}
	q.known[in.RequestID] = struct{}{}
	q.pending = append(q.pending, activities.FrontierQueueItem{
		RequestID: in.RequestID,
		URL:       in.URL,
		Payload:   map[string]any{"depth": float64(in.Depth)},
		Attempt:   1,
	})
	return nil
}

func (q *durableFakeQueue) pop(_ context.Context, _ activities.FrontierPopInput) (*activities.FrontierQueueItem, error) {
	if len(q.pending) == 0 {
		return nil, nil
	}
	item := q.pending[0]
	q.pending = q.pending[1:]
	return &item, nil
}

func (q *durableFakeQueue) ack(_ context.Context, in activities.FrontierAckInput) error {
	q.acked[in.RequestID]++
	return nil
}

// TestCrawlJobDurable_EnqueuesEveryLinkWithOneActivityEach is the first test of
// the production frontier loop (WS14.S0). Production runs the durable variant
// when the edge credentials are set (activities.New sets DurableFrontier), and
// until now no test set DurableFrontier to true. The graph below has a cycle and
// a shared target, so the test covers both the one-activity-per-link rule and
// the queue's own deduplication.
func TestCrawlJobDurable_EnqueuesEveryLinkWithOneActivityEach(t *testing.T) {
	env, a := newEnv(t)
	a.DurableFrontier = true

	const root = "https://durable.example/"
	links := map[string][]string{
		root:                        {"https://durable.example/a", "https://durable.example/b"},
		"https://durable.example/a": {root, "https://durable.example/c"},
		"https://durable.example/b": {"https://durable.example/c"},
		"https://durable.example/c": nil,
	}

	q := newDurableFakeQueue()
	fetched := map[string]int{}

	env.OnActivity(a.FrontierEnqueue, mock.Anything, mock.Anything).Return(q.enqueue)
	env.OnActivity(a.FrontierPop, mock.Anything, mock.Anything).Return(q.pop)
	env.OnActivity(a.FrontierAck, mock.Anything, mock.Anything).Return(q.ack)
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			fetched[in.URL]++
			return activities.RunPageResult{
				Status:      200,
				Fingerprint: "blake3:" + in.URL,
				Links:       links[in.URL],
			}, nil
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)

	env.ExecuteWorkflow(CrawlJobWF, CrawlJobInput{
		RunID:    "run_s0_durable",
		OrgID:    "org_s0",
		Seeds:    []string{root},
		MaxDepth: 5,
		MaxPages: 100,
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())

	// One enqueue activity for the seed and one per link of each fetched page:
	// root (1 seed + 2 links), a (2 links), b (1 link), c (0 links).
	require.Equal(t, 6, q.enqueued, "one FrontierEnqueue activity per seed and per link")

	// Each distinct URL is fetched exactly once; the queue drops the repeats.
	require.Len(t, fetched, 4)
	for url, n := range fetched {
		require.Equal(t, 1, n, "page %s fetched %d times", url, n)
	}

	// Each popped item is acknowledged exactly once.
	require.Len(t, q.acked, 4)
	for id, n := range q.acked {
		require.Equal(t, 1, n, "request %s acked %d times", id, n)
	}
	require.Empty(t, q.pending, "the frontier is drained when the crawl completes")
}
