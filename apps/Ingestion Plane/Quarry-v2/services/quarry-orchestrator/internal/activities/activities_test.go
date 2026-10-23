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

// WS6.C-e: the orchestrator declares the job kind of a page on the run_page body, so the
// edge meters a crawl page and no other. An empty kind (a scrape or a monitor) leaves the
// key out, so the body is byte-identical to the one sent before the field existed.
func TestRunPageDeclaresJobKind(t *testing.T) {
	var mu sync.Mutex
	var bodies []map[string]any
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/run_page", func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var body map[string]any
		_ = json.Unmarshal(raw, &body)
		mu.Lock()
		bodies = append(bodies, body)
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":200,"fingerprint":"blake3:fresh","links":[]}`))
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)

	a := New(Config{RuntimeBaseURL: srv.URL, RuntimeAuthToken: "test-token"})
	for _, kind := range []string{"crawl", "batch", ""} {
		_, err := a.RunPage(context.Background(), RunPageInput{
			RunID: "run_test",
			URL:   "https://example.com/page",
			OrgID: "org_a",
			Kind:  kind,
		})
		require.NoError(t, err)
	}

	mu.Lock()
	defer mu.Unlock()
	require.Len(t, bodies, 3)
	require.Equal(t, "crawl", bodies[0]["kind"])
	require.Equal(t, "batch", bodies[1]["kind"])
	_, declared := bodies[2]["kind"]
	require.False(t, declared, "an empty kind is not declared on the body")
}
