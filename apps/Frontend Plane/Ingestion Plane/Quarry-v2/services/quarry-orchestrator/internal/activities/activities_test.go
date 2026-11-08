package activities

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
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

// ---------------------------------------------------------------------------
// WS7.G 6b: the policy snapshot a durable job's pages carry, and the run-binding signature
// that covers it. The Rust side holds the same document and the same constants
// (quarry-edge internal_auth.rs: GOLDEN_SNAPSHOT, GOLDEN_CANONICAL, GOLDEN_SHA256, GOLDEN_SIG),
// so a change to either writer breaks a test on both sides.
// ---------------------------------------------------------------------------

// goldenPolicySnapshot has its keys out of canonical order and whitespace in it on purpose.
const goldenPolicySnapshot = `{
  "v": 1,
  "proxy_max": "none",
  "robots_max": "respect",
  "modes": {"search.paid_provider": "disabled", "browser.act.input": "disabled"},
  "deny": ["bad.example", "evil.test"],
  "allow": [["docs.example.org", "example.com"], ["example.com"]],
  "org_rev": 7
}`

const (
	goldenCanonical = `{"allow":[["docs.example.org","example.com"],["example.com"]],"deny":["bad.example","evil.test"],"modes":{"browser.act.input":"disabled","search.paid_provider":"disabled"},"org_rev":7,"proxy_max":"none","robots_max":"respect","v":1}`
	goldenSHA256    = "eac471399cd4fdd009a071ed0ce1605c547b4c26b1a77adb140b1a913b78782c"
	goldenSecret    = "0123456789abcdef0123456789abcdef"
	goldenURL       = "https://example.com/pricing"
	goldenSig       = "e85e8d3b76888dbce0f63b66035a3fb7d79280d2ec04f86017852256295e888a"
)

func TestPolicySnapshotGolden(t *testing.T) {
	canonical, err := canonicalPolicySnapshot(json.RawMessage(goldenPolicySnapshot))
	require.NoError(t, err)
	require.Equal(t, goldenCanonical, canonical)

	sum, err := policySnapshotSHA256(json.RawMessage(goldenPolicySnapshot))
	require.NoError(t, err)
	require.Equal(t, goldenSHA256, sum)

	sig, err := runBindingSigExtras(goldenSecret, "org_a", goldenURL, json.RawMessage(goldenPolicySnapshot))
	require.NoError(t, err)
	require.Equal(t, goldenSig, sig)
}

func TestRunBindingSigExtrasWithoutASnapshotIsTheTwoFieldString(t *testing.T) {
	plain := runBindingSig(goldenSecret, "org_a", goldenURL)
	for name, snapshot := range map[string]json.RawMessage{
		"absent": nil,
		"empty":  json.RawMessage(""),
		"null":   json.RawMessage(" null "),
	} {
		got, err := runBindingSigExtras(goldenSecret, "org_a", goldenURL, snapshot)
		require.NoError(t, err, name)
		require.Equal(t, plain, got, name)
	}
	extended, err := runBindingSigExtras(goldenSecret, "org_a", goldenURL, json.RawMessage(goldenPolicySnapshot))
	require.NoError(t, err)
	require.NotEqual(t, plain, extended, "the snapshot is part of what is signed")

	// A loosened copy (one deny host dropped) does not sign the same string.
	loosened := strings.Replace(goldenPolicySnapshot, `"bad.example", `, "", 1)
	looseSig, err := runBindingSigExtras(goldenSecret, "org_a", goldenURL, json.RawMessage(loosened))
	require.NoError(t, err)
	require.NotEqual(t, extended, looseSig)

	// Key order and whitespace do not matter, only the content does.
	reordered := `{"org_rev":7,"v":1,"allow":[["docs.example.org","example.com"],["example.com"]],"modes":{"browser.act.input":"disabled","search.paid_provider":"disabled"},"robots_max":"respect","proxy_max":"none","deny":["bad.example","evil.test"]}`
	sameSig, err := runBindingSigExtras(goldenSecret, "org_a", goldenURL, json.RawMessage(reordered))
	require.NoError(t, err)
	require.Equal(t, extended, sameSig)
}

// runPageRecorder is an edge that records the body and signature header of each run_page call.
type runPageRecorder struct {
	mu     sync.Mutex
	bodies []map[string]any
	sigs   []string
}

func (r *runPageRecorder) serve(t *testing.T) *httptest.Server {
	t.Helper()
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/run_page", func(w http.ResponseWriter, req *http.Request) {
		raw, _ := io.ReadAll(req.Body)
		var body map[string]any
		_ = json.Unmarshal(raw, &body)
		r.mu.Lock()
		r.bodies = append(r.bodies, body)
		r.sigs = append(r.sigs, req.Header.Get("X-Quarry-Run-Sig"))
		r.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"status":200,"fingerprint":"blake3:fresh","links":[]}`))
	})
	srv := httptest.NewServer(mux)
	t.Cleanup(srv.Close)
	return srv
}

func TestRunPageCarriesThePolicySnapshotAndSignsItsHash(t *testing.T) {
	rec := &runPageRecorder{}
	srv := rec.serve(t)
	a := New(Config{RuntimeBaseURL: srv.URL, RuntimeAuthToken: "test-token", EdgeRunSecret: goldenSecret})

	for _, snapshot := range []json.RawMessage{json.RawMessage(goldenPolicySnapshot), nil, json.RawMessage("null")} {
		_, err := a.RunPage(context.Background(), RunPageInput{
			RunID: "run_test", URL: goldenURL, OrgID: "org_a", Kind: "crawl", PolicySnapshot: snapshot,
		})
		require.NoError(t, err)
	}

	rec.mu.Lock()
	defer rec.mu.Unlock()
	require.Len(t, rec.bodies, 3)

	// With a snapshot: it is in the body as the object it is, and the signature is the golden one.
	carried, ok := rec.bodies[0]["policy_snapshot"].(map[string]any)
	require.True(t, ok, "the snapshot rides in the body as a JSON object")
	require.EqualValues(t, 1, carried["v"])
	require.Equal(t, goldenSig, rec.sigs[0])

	// Without one (absent or null): no key, and the signature is the plain two-field one.
	for i := 1; i < 3; i++ {
		_, declared := rec.bodies[i]["policy_snapshot"]
		require.False(t, declared, "no snapshot, no key")
		require.Equal(t, runBindingSig(goldenSecret, "org_a", goldenURL), rec.sigs[i])
	}
}

func TestRunPageRefusesASnapshotThatIsNotJSONWithoutCallingTheEdge(t *testing.T) {
	rec := &runPageRecorder{}
	srv := rec.serve(t)
	a := New(Config{RuntimeBaseURL: srv.URL, RuntimeAuthToken: "test-token", EdgeRunSecret: goldenSecret})

	_, err := a.RunPage(context.Background(), RunPageInput{
		RunID: "run_test", URL: goldenURL, OrgID: "org_a", PolicySnapshot: json.RawMessage(`{"v":`),
	})
	require.Error(t, err)
	rec.mu.Lock()
	defer rec.mu.Unlock()
	require.Empty(t, rec.bodies, "a snapshot that cannot be signed must not reach the edge")
}
