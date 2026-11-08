package workflows

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/errs"
)

func TestStatusClassIsClosed(t *testing.T) {
	for status, want := range map[uint16]string{
		200: "2xx", 204: "2xx", 301: "3xx", 404: "4xx", 429: "4xx",
		500: "5xx", 503: "5xx", 0: "other", 100: "other", 700: "other",
	} {
		require.Equal(t, want, statusClass(status), "status %d", status)
	}
}

func TestStatisticsCountPagesFailuresAndDrivers(t *testing.T) {
	start := time.Date(2026, 10, 16, 12, 0, 0, 0, time.UTC)
	stats := newJobStatistics(start)
	stats.recordPage(activities.RunPageResult{Status: 200, Driver: "static"})
	stats.recordPage(activities.RunPageResult{Status: 200, Driver: "browser"})
	stats.recordPage(activities.RunPageResult{Status: 404, Driver: "a-driver-this-build-has-never-heard-of"})
	stats.recordPage(activities.RunPageResult{Status: 502}) // no driver reported
	stats.recordFailure(errs.New(errs.CategoryClient4xx, "test", errors.New("boom")).Temporal())
	stats.recordFailure(errs.New(errs.CategoryNetwork, "test", errors.New("boom")))
	stats.recordFailure(errors.New("not classified at all"))

	got := stats.payload(4, 3, start.Add(1500*time.Millisecond))
	require.Equal(t, uint32(4), got["pages_visited"])
	require.Equal(t, uint32(3), got["pages_failed"])
	require.Equal(t, int64(1500), got["duration_ms"])
	require.Equal(t, map[string]uint32{"2xx": 2, "4xx": 1, "5xx": 1}, got["status_classes"])
	require.Equal(t, map[string]uint32{"static": 1, "browser": 1, "other": 1}, got["drivers"])
	require.Equal(t, map[string]uint32{"client_4xx": 1, "network": 1, "unknown": 1}, got["error_classes"])
}

func TestStatisticsOmitEmptyBreakdownsAndDoNotAliasState(t *testing.T) {
	stats := newJobStatistics(time.Unix(0, 0))
	empty := stats.payload(0, 0, time.Unix(2, 0))
	for _, key := range []string{"status_classes", "drivers", "error_classes"} {
		_, present := empty[key]
		require.False(t, present, key)
	}
	require.Equal(t, int64(2000), empty["duration_ms"])

	stats.recordPage(activities.RunPageResult{Status: 200, Driver: "static"})
	first := stats.payload(1, 0, time.Unix(3, 0))
	first["status_classes"].(map[string]uint32)["2xx"] = 99
	require.Equal(t, uint32(1), stats.payload(1, 0, time.Unix(3, 0))["status_classes"].(map[string]uint32)["2xx"])
}

func TestStatisticsContainNoUrlOrErrorText(t *testing.T) {
	stats := newJobStatistics(time.Unix(0, 0))
	stats.recordPage(activities.RunPageResult{
		Status: 200, Driver: "static", Title: "Private client page", Fingerprint: "blake3:secret",
		Links: []string{"https://client.example/secret-path"},
	})
	stats.recordFailure(errs.New(errs.CategoryBlocklist, "RunPage",
		errors.New("GET https://client.example/secret-path: blocked by tenant list")).Temporal())
	encoded, err := json.Marshal(stats.payload(1, 1, time.Unix(5, 0)))
	require.NoError(t, err)
	for _, leak := range []string{"client.example", "secret-path", "Private client page", "blake3", "blocked by tenant", "GET "} {
		require.NotContains(t, string(encoded), leak)
	}
}

func TestCrawlJobWF_TerminalEventCarriesStatistics(t *testing.T) {
	env, a := newEnv(t)

	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			switch in.URL {
			case "https://crawl.example/ok":
				return activities.RunPageResult{Status: 200, Driver: "static"}, nil
			case "https://crawl.example/js":
				return activities.RunPageResult{Status: 200, Driver: "browser"}, nil
			case "https://crawl.example/gone":
				return activities.RunPageResult{Status: 404, Driver: "static"}, nil
			default:
				return activities.RunPageResult{},
					errs.New(errs.CategoryClient4xx, "RunPage", errors.New("forbidden by origin")).Temporal()
			}
		},
	)
	var completed map[string]any
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ string, evt quarrycontracts.Event) error {
			if evt.Type == quarrycontracts.EvtRunCompleted {
				completed = evt.Payload
			}
			return nil
		},
	)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)

	env.ExecuteWorkflow(CrawlJobWF, CrawlJobInput{
		RunID: "run_stats",
		Seeds: []string{
			"https://crawl.example/ok", "https://crawl.example/js",
			"https://crawl.example/gone", "https://crawl.example/denied",
		},
		MaxDepth: 1,
		MaxPages: 50,
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.NotNil(t, completed)
	statistics, ok := completed["statistics"].(map[string]any)
	require.True(t, ok, "run_completed carries statistics: %v", completed)
	require.EqualValues(t, 3, statistics["pages_visited"])
	require.EqualValues(t, 1, statistics["pages_failed"])
	require.Equal(t, map[string]any{"2xx": float64(2), "4xx": float64(1)}, statistics["status_classes"])
	require.Equal(t, map[string]any{"static": float64(2), "browser": float64(1)}, statistics["drivers"])
	require.Equal(t, map[string]any{"client_4xx": float64(1)}, statistics["error_classes"])
	require.Contains(t, statistics, "duration_ms")
	encoded, err := json.Marshal(statistics)
	require.NoError(t, err)
	require.NotContains(t, string(encoded), "crawl.example")
	require.NotContains(t, string(encoded), "forbidden by origin")
}

func TestBatchJobWF_TerminalEventCarriesStatistics(t *testing.T) {
	env, a := newEnv(t)

	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			if in.URL == "https://b.example" {
				return activities.RunPageResult{},
					errs.New(errs.CategoryBlocklist, "RunPage", errors.New("blocked")).Temporal()
			}
			return activities.RunPageResult{Status: 200, Driver: "tls"}, nil
		},
	)
	var completed map[string]any
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(
		func(_ context.Context, _ string, evt quarrycontracts.Event) error {
			if evt.Type == quarrycontracts.EvtRunCompleted {
				completed = evt.Payload
			}
			return nil
		},
	)

	env.ExecuteWorkflow(BatchJobWF, BatchJobInput{
		RunID: "run_batch_stats",
		URLs:  []string{"https://a.example", "https://b.example", "https://c.example"},
	}, a)

	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	statistics, ok := completed["statistics"].(map[string]any)
	require.True(t, ok, "%v", completed)
	require.EqualValues(t, 2, statistics["pages_visited"])
	require.EqualValues(t, 1, statistics["pages_failed"])
	require.Equal(t, map[string]any{"tls": float64(2)}, statistics["drivers"])
	require.Equal(t, map[string]any{"blocklist": float64(1)}, statistics["error_classes"])
}
