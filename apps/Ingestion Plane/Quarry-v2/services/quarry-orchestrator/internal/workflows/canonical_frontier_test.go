package workflows

import (
	"context"
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"
	"go.temporal.io/sdk/workflow"

	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// W3 E3.7.6: the canonical frontier identity on the enqueue paths, behind
// QUARRY_ORCHESTRATOR__CANONICAL_FRONTIER (default off).

func setCanonicalFrontierFlag(t *testing.T, value string) {
	t.Helper()
	prev := canonicalFrontierEnv
	canonicalFrontierEnv = func() string { return value }
	t.Cleanup(func() { canonicalFrontierEnv = prev })
}

func TestParseCanonicalFrontier(t *testing.T) {
	for _, on := range []string{"1", "true", " ON ", "yes"} {
		require.True(t, parseCanonicalFrontier(on), on)
	}
	for _, off := range []string{"", "0", "off", "false", "maybe"} {
		require.False(t, parseCanonicalFrontier(off), off)
	}
}

func TestFrontierTarget_OffKeepsTheRawURLAndID(t *testing.T) {
	id, target := frontierTarget("run_1", "HTTPS://Example.com/a#frag", false)
	require.Equal(t, "HTTPS://Example.com/a#frag", target)
	require.Equal(t, durableFrontierRequestID("run_1", "HTTPS://Example.com/a#frag"), id)
}

func TestFrontierTarget_OnUsesTheCanonicalFormAndItsID(t *testing.T) {
	id, target := frontierTarget("run_1", "HTTPS://Example.com:443/a#frag", true)
	require.Equal(t, "https://example.com/a", target)
	require.Equal(t, activities.FrontierRequestID("run_1", "https://example.com/a"), id)
	require.Equal(t, activities.FrontierCanonicalRequestID("run_1", "HTTPS://Example.com:443/a#frag"), id)
}

func TestFrontierTarget_OnKeepsAURLTheMirrorRefuses(t *testing.T) {
	raw := "https://user@example.com/a"
	id, target := frontierTarget("run_1", raw, true)
	require.Equal(t, raw, target)
	require.Equal(t, durableFrontierRequestID("run_1", raw), id)
}

// canonicalCrawl runs a durable crawl whose root links to three spellings of one page.
func canonicalCrawl(t *testing.T, version workflow.Version) map[string]int {
	t.Helper()
	env, a := newEnv(t)
	a.DurableFrontier = true
	env.OnGetVersion(canonicalFrontierVersion, workflow.DefaultVersion, 1).Return(version)

	const root = "https://canonical.example/"
	links := map[string][]string{
		root: {
			"https://canonical.example/a",
			"https://canonical.example/a#section",
			"HTTPS://Canonical.Example:443/a",
		},
	}
	q := newDurableFakeQueue()
	fetched := map[string]int{}
	env.OnActivity(a.FrontierEnqueue, mock.Anything, mock.Anything).Return(q.enqueue)
	env.OnActivity(a.FrontierPop, mock.Anything, mock.Anything).Return(q.pop)
	env.OnActivity(a.FrontierAck, mock.Anything, mock.Anything).Return(q.ack)
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(
		func(_ context.Context, in activities.RunPageInput) (activities.RunPageResult, error) {
			fetched[in.URL]++
			return activities.RunPageResult{Status: 200, Fingerprint: "blake3:" + in.URL, Links: links[in.URL]}, nil
		},
	)
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).Return(nil)
	env.OnActivity(a.Checkpoint, mock.Anything, mock.Anything).Return(nil)

	env.ExecuteWorkflow(CrawlJobWF, CrawlJobInput{
		RunID:    "run_e376_canonical",
		OrgID:    "org_e376",
		Seeds:    []string{root},
		MaxDepth: 3,
		MaxPages: 50,
	}, a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	return fetched
}

func TestCrawlJobDurable_CanonicalFrontierFetchesOneSpellingOnce(t *testing.T) {
	setCanonicalFrontierFlag(t, "on")
	fetched := canonicalCrawl(t, workflow.Version(1))
	require.Equal(t, 1, fetched["https://canonical.example/a"])
	require.Len(t, fetched, 2, "the root and one canonical page")
}

func TestCrawlJobDurable_CanonicalFrontierOffKeepsEverySpelling(t *testing.T) {
	setCanonicalFrontierFlag(t, "")
	fetched := canonicalCrawl(t, workflow.Version(1))
	require.Len(t, fetched, 4, "the root and three raw spellings, as before")
}

func TestCrawlJobDurable_CanonicalFrontierOldHistoryNeverReadsTheFlag(t *testing.T) {
	setCanonicalFrontierFlag(t, "on")
	fetched := canonicalCrawl(t, workflow.DefaultVersion)
	require.Len(t, fetched, 4, "a history before the marker keeps the raw ids")
}
