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

// recordCapture plays the runtime (run_page) and the edge record route, keeping
// the raw bytes of every record body so a test can assert exactly which keys
// the orchestrator declared.
func recordCapture(t *testing.T) (*httptest.Server, func() [][]byte) {
	t.Helper()
	var mu sync.Mutex
	var bodies [][]byte
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/run_page", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":200,"fingerprint":"blake3:fresh","links":[]}`))
	})
	mux.HandleFunc("/v1/internal/change/record", func(w http.ResponseWriter, r *http.Request) {
		b, _ := io.ReadAll(r.Body)
		mu.Lock()
		bodies = append(bodies, b)
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":"new","changed":false,"fingerprint":"blake3:fresh"}`))
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv, func() [][]byte {
		mu.Lock()
		defer mu.Unlock()
		return append([][]byte(nil), bodies...)
	}
}

// WS4.H: the posture stamped on the schedule is forwarded verbatim on the
// record body, true and an explicit false alike.
func TestCheckChangeSendsStampedZdr(t *testing.T) {
	yes, no := true, false
	for _, tc := range []struct {
		name string
		in   *bool
		want bool
	}{{"zdr", &yes, true}, {"standard", &no, false}} {
		t.Run(tc.name, func(t *testing.T) {
			srv, bodies := recordCapture(t)
			_, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
				RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p", Zdr: tc.in,
			})
			require.NoError(t, err)
			got := bodies()
			require.Len(t, got, 1)
			var m map[string]any
			require.NoError(t, json.Unmarshal(got[0], &m))
			v, present := m["zdr"]
			require.True(t, present, "the stamp must be declared on the record body: %s", got[0])
			require.Equal(t, tc.want, v)
		})
	}
}

// A schedule that predates the stamp sends a body byte-identical to the one
// shipped before this change: no zdr key at all.
func TestCheckChangeOmitsZdrWhenUnstamped(t *testing.T) {
	srv, bodies := recordCapture(t)
	_, err := newChangeActivities(srv).CheckChange(context.Background(), CheckChangeInput{
		RunID: "run_1", OrgID: "org_a", URL: "https://example.com/p",
	})
	require.NoError(t, err)
	got := bodies()
	require.Len(t, got, 1)
	require.JSONEq(t, `{"org_id":"org_a","url":"https://example.com/p","fresh_fingerprint":"blake3:fresh","run_id":"run_1"}`, string(got[0]))
	var m map[string]any
	require.NoError(t, json.Unmarshal(got[0], &m))
	require.NotContains(t, m, "zdr")
}
