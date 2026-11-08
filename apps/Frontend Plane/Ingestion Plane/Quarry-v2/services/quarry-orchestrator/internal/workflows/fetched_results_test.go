package workflows

import (
	"testing"

	"github.com/stretchr/testify/mock"
	"github.com/stretchr/testify/require"

	"github.com/triodelab/quarry-v2/pkg/quarrycontracts"
	"github.com/triodelab/quarry-v2/services/quarry-orchestrator/internal/activities"
)

// pageFetchedPayload runs a one-URL scrape workflow whose RunPage returns res and
// returns the payload of the page_fetched event it emitted.
func pageFetchedPayload(t *testing.T, res activities.RunPageResult) map[string]any {
	t.Helper()
	env, a := newEnv(t)
	env.OnActivity(a.RunPage, mock.Anything, mock.Anything).Return(res, nil).Once()

	var fetched map[string]any
	env.OnActivity(a.EmitEvent, mock.Anything, mock.Anything, mock.Anything).
		Run(func(args mock.Arguments) {
			evt, ok := args.Get(2).(quarrycontracts.Event)
			if !ok {
				if p, okp := args.Get(2).(*quarrycontracts.Event); okp && p != nil {
					evt, ok = *p, true
				}
			}
			if ok && evt.Type == quarrycontracts.EvtPageFetched {
				fetched = evt.Payload
			}
		}).
		Return(nil)

	env.ExecuteWorkflow(ScrapeJobWF, ScrapeJobInput{RunID: "run_results_1", URL: "https://example.com/p"}, a)
	require.True(t, env.IsWorkflowCompleted())
	require.NoError(t, env.GetWorkflowError())
	require.NotNil(t, fetched, "page_fetched was emitted")
	return fetched
}

func TestPageFetchedCarriesTheResultFieldsWhenRunPageReturnsThem(t *testing.T) {
	got := pageFetchedPayload(t, activities.RunPageResult{
		Status:             200,
		Fingerprint:        "blake3:abc",
		ContentType:        "text/html",
		Title:              "Home",
		TitleSource:        "html",
		WordCount:          120,
		Driver:             "browser",
		MarkdownArtifactID: "art_01MD",
		SourceArtifactID:   "art_01SRC",
		Bytes:              4096,
	})
	require.Equal(t, "https://example.com/p", got["url"])
	require.EqualValues(t, 200, got["status"])
	require.Equal(t, "art_01MD", got["markdown_artifact_id"])
	require.Equal(t, "art_01SRC", got["source_artifact_id"])
	require.EqualValues(t, 4096, got["bytes"])
	require.EqualValues(t, 120, got["word_count"])
	require.Equal(t, "browser", got["driver"])
}

func TestPageFetchedOmitsTheResultFieldsWhenRunPageReturnsNone(t *testing.T) {
	// ZDR, an older runtime and a non-HTML response all return none of them: the payload keeps
	// the shape it had before INNH-12, with no empty keys.
	got := pageFetchedPayload(t, activities.RunPageResult{Status: 200, Fingerprint: "blake3:abc"})
	for _, key := range []string{"markdown_artifact_id", "source_artifact_id", "bytes", "word_count", "driver"} {
		require.NotContains(t, got, key)
	}
	require.Contains(t, got, "url")
	require.Contains(t, got, "title")
}

func TestAddFetchedResultFieldsIsContentFree(t *testing.T) {
	m := map[string]any{}
	addFetchedResultFields(m, activities.RunPageResult{
		Excerpt: "page text must never ride page_fetched", Summary: "nor this", MarkdownArtifactID: "art_1",
	})
	require.Equal(t, map[string]any{"markdown_artifact_id": "art_1"}, m)
}
