package activities

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"sync"
	"testing"

	"github.com/stretchr/testify/require"
)

// pageAndRecord plays the runtime run_page route with a canned body and the edge
// record route, keeping the raw bytes of every record body (INNH-4).
func pageAndRecord(t *testing.T, runPageBody string) (*httptest.Server, func() [][]byte) {
	t.Helper()
	var mu sync.Mutex
	var bodies [][]byte
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/run_page", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(runPageBody))
	})
	mux.HandleFunc("/v1/internal/change/record", func(w http.ResponseWriter, r *http.Request) {
		b, _ := io.ReadAll(r.Body)
		mu.Lock()
		bodies = append(bodies, b)
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":"changed","changed":true,"fingerprint":"blake3:fresh","baseline_id":"bln_1","diff_id":"diff_1"}`))
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv, func() [][]byte {
		mu.Lock()
		defer mu.Unlock()
		return append([][]byte(nil), bodies...)
	}
}

// run_page's markdown artifact id rides on the record body, so the edge can keep it on the
// baseline it saves. The result the workflow reads (baseline and diff ids) is unchanged.
func TestCheckChangeForwardsTheMarkdownArtifactID(t *testing.T) {
	srv, bodies := pageAndRecord(t, `{"status":200,"fingerprint":"blake3:fresh","links":[],"markdown_artifact_id":"art_01MD"}`)
	out, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p",
	})
	require.NoError(t, err)
	require.Equal(t, "diff_1", out.DiffID)
	require.Equal(t, "bln_1", out.BaselineID)
	got := bodies()
	require.Len(t, got, 1)
	require.JSONEq(t, `{"org_id":"org_a","url":"https://example.com/p","fresh_fingerprint":"blake3:fresh","run_id":"run_1","markdown_artifact_id":"art_01MD"}`, string(got[0]))
}

// A runtime that predates the field (or a ZDR run, which stores no page) returns none: the
// record body is then byte-identical to the one shipped before INNH-4, key and all.
func TestCheckChangeOmitsTheMarkdownArtifactIDWhenRunPageReturnsNone(t *testing.T) {
	srv, bodies := pageAndRecord(t, `{"status":200,"fingerprint":"blake3:fresh","links":[]}`)
	_, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p",
	})
	require.NoError(t, err)
	got := bodies()
	require.Len(t, got, 1)
	require.JSONEq(t, `{"org_id":"org_a","url":"https://example.com/p","fresh_fingerprint":"blake3:fresh","run_id":"run_1"}`, string(got[0]))
	var m map[string]any
	require.NoError(t, json.Unmarshal(got[0], &m))
	require.NotContains(t, m, "markdown_artifact_id")
}

// RunPage itself decodes the field, for the callers that are not the change monitor.
func TestRunPageDecodesTheMarkdownArtifactID(t *testing.T) {
	srv, _ := pageAndRecord(t, `{"status":200,"fingerprint":"blake3:fresh","links":[],"markdown_artifact_id":"art_01MD"}`)
	page, err := newChangeActivities(srv).RunPage(context.Background(), RunPageInput{
		RunID: "run_1", URL: "https://example.com/p", OrgID: "org_a",
	})
	require.NoError(t, err)
	require.Equal(t, "art_01MD", page.MarkdownArtifactID)
}

// INNH-12: RunPage also decodes the source artifact id and the markdown size, which the
// workflow puts on page_fetched.
func TestRunPageDecodesTheSourceArtifactAndBytes(t *testing.T) {
	srv, _ := pageAndRecord(t, `{"status":200,"fingerprint":"blake3:fresh","links":[],"markdown_artifact_id":"art_01MD","source_artifact_id":"art_01SRC","bytes":2048}`)
	page, err := newChangeActivities(srv).RunPage(context.Background(), RunPageInput{
		RunID: "run_1", URL: "https://example.com/p", OrgID: "org_a",
	})
	require.NoError(t, err)
	require.Equal(t, "art_01SRC", page.SourceArtifactID)
	require.Equal(t, uint64(2048), page.Bytes)
}
