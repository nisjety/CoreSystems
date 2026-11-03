package activities

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/stretchr/testify/require"
)

// E3.7.6: the canonical frontier identity. The expectations follow the Rust helper's own
// table (quarry-transform/tests/canonical_url.rs), so the Go mirror is held to the same rules.

func TestCanonicalFrontierURL_FoldsWhatTheRustHelperFolds(t *testing.T) {
	cases := []struct{ in, want string }{
		{"HTTP://Example.COM:80/a/./b/../c?x=1#frag", "http://example.com/a/c?x=1"},
		{"https://example.com:443", "https://example.com/"},
		{"https://example.com.", "https://example.com/"},
		{"https://Example.com/%7Euser", "https://example.com/~user"},
		{"https://example.com/a%41b", "https://example.com/aAb"},
		{"https://example.com/a%2fb", "https://example.com/a%2Fb"},
		{"https://example.com/?", "https://example.com/"},
		{"https://example.com/a?b=%7e", "https://example.com/a?b=~"},
		{"https://example.com/a?b=1&a=2", "https://example.com/a?b=1&a=2"},
		{"https://example.com/a/../../b", "https://example.com/b"},
		{"https://example.com/%2e%2E/x", "https://example.com/x"},
		{"http://example.com:8080/a", "http://example.com:8080/a"},
		{"https://example.com/a/", "https://example.com/a/"},
		{"https://example.com/www.index.html", "https://example.com/www.index.html"},
	}
	for _, c := range cases {
		got, ok := CanonicalFrontierURL(c.in)
		require.True(t, ok, c.in)
		require.Equal(t, c.want, got, c.in)
	}
}

func TestCanonicalFrontierURL_KeepsDistinctResourcesDistinct(t *testing.T) {
	pairs := [][2]string{
		{"https://example.com/a", "https://example.com/a/"},
		{"https://example.com/", "https://www.example.com/"},
		{"https://example.com/index.html", "https://example.com/"},
		{"https://example.com/p?a=1&b=2", "https://example.com/p?b=2&a=1"},
	}
	for _, p := range pairs {
		a, okA := CanonicalFrontierURL(p[0])
		b, okB := CanonicalFrontierURL(p[1])
		require.True(t, okA && okB, p)
		require.NotEqual(t, a, b, p)
	}
}

func TestCanonicalFrontierURL_IsIdempotent(t *testing.T) {
	for _, in := range []string{
		"HTTP://Example.COM:80/a/./b/../c?x=1#frag",
		"https://Example.com/%7Euser?b=%41&a=%2f",
		"https://example.com./a/",
		"https://example.com/p?utm_source=x&b=2&a=1",
	} {
		once, ok := CanonicalFrontierURL(in)
		require.True(t, ok, in)
		twice, ok := CanonicalFrontierURL(once)
		require.True(t, ok, once)
		require.Equal(t, once, twice, in)
	}
}

func TestCanonicalFrontierURL_RefusesWhatTheMirrorDoesNotCover(t *testing.T) {
	long := "https://example.com/" + strings.Repeat("a", frontierCanonicalMaxBytes)
	for _, in := range []string{
		"",
		"not a url",
		"https://",
		"ftp://example.com/",
		"javascript:alert(1)",
		"mailto:someone@example.com",
		"http://user:pw@example.com/",
		"http://example.com@evil.test/",
		"https://exämple.com/",
		"https://example.com/a b",
		"https://example.com/a\\b",
		"https://example.com/a|b",
		"https://example.com/p?q='x'",
		"http://[::1]/",
		"https://127.0.0.1/",
		"https://example.123/",
		"https://0x7f.1/",
		"https://example..com/",
		"https://.example.com/",
		"https://-a.example.com/",
		"https://xn--exmple-cua.com/",
		"https://example.com:/",
		"https://example.com:99999/",
		"https://example.com:x/",
		long,
	} {
		_, ok := CanonicalFrontierURL(in)
		require.False(t, ok, in)
	}
}

func TestFrontierCanonicalRequestID_IsTheRawIDForAURLThatIsAlreadyCanonical(t *testing.T) {
	for _, u := range []string{"https://site.test/", "https://site.test/p/0", "https://site.test/a/"} {
		require.Equal(t, FrontierRequestID("run_1", u), FrontierCanonicalRequestID("run_1", u), u)
	}
}

func TestFrontierCanonicalRequestID_GivesOneIDToTheSpellingsOfOnePage(t *testing.T) {
	want := FrontierCanonicalRequestID("run_1", "https://site.test/a")
	for _, u := range []string{"HTTPS://Site.TEST:443/a#top", "https://site.test/a?", "https://site.test/%61"} {
		require.Equal(t, want, FrontierCanonicalRequestID("run_1", u), u)
	}
	require.NotEqual(t, want, FrontierCanonicalRequestID("run_1", "https://site.test/a/"), "a trailing slash is another resource")
	require.NotEqual(t, want, FrontierCanonicalRequestID("run_2", "https://site.test/a"), "each run keeps its own ids")
}

func TestFrontierCanonicalRequestID_KeepsTheRawIDWhenTheMirrorRefuses(t *testing.T) {
	raw := "https://exämple.test/a"
	require.Equal(t, FrontierRequestID("run_1", raw), FrontierCanonicalRequestID("run_1", raw))
}

func TestFrontierSeenRequestIDs_ARedirectMarksTheRequestedAndTheFinalID(t *testing.T) {
	got := FrontierSeenRequestIDs("run_1", "http://site.test/a", "https://site.test/b")
	require.Equal(t, []string{
		FrontierRequestID("run_1", "http://site.test/a"),
		FrontierRequestID("run_1", "https://site.test/b"),
	}, got)
}

func TestFrontierSeenRequestIDs_OneIDWhenTheFinalURLIsTheSameResourceOrMissing(t *testing.T) {
	require.Len(t, FrontierSeenRequestIDs("run_1", "https://site.test/a", "https://site.test/a#x"), 1)
	require.Len(t, FrontierSeenRequestIDs("run_1", "https://site.test/a", ""), 1)
	require.Len(t, FrontierSeenRequestIDs("run_1", "https://site.test/a", "  "), 1)
}

// fakeSeenEdge answers the queue's seen route and records what it was sent.
type fakeSeenEdge struct {
	mu    sync.Mutex
	paths []string
	sent  [][]string
}

func (f *fakeSeenEdge) handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/internal/queues/", func(w http.ResponseWriter, r *http.Request) {
		raw, _ := io.ReadAll(r.Body)
		var body struct {
			RequestIDs []string `json:"request_ids"`
		}
		_ = json.Unmarshal(raw, &body)
		f.mu.Lock()
		f.paths = append(f.paths, r.URL.Path)
		f.sent = append(f.sent, body.RequestIDs)
		f.mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(fmt.Sprintf(`{"data":{"accepted":%d}}`, len(body.RequestIDs))))
	})
	return mux
}

func newSeenActivities(t *testing.T, edge *fakeSeenEdge) *Activities {
	t.Helper()
	srv := httptest.NewServer(edge.handler())
	t.Cleanup(srv.Close)
	return New(Config{
		RuntimeBaseURL:   srv.URL,
		RuntimeAuthToken: "runtime-token",
		EdgeBaseURL:      srv.URL,
		EdgeAuthToken:    "edge-token",
		EdgeRunSecret:    "run-secret",
	})
}

func TestFrontierMarkSeen_SendsTheIDsInBatchesAndCountsWhatTheQueueRecords(t *testing.T) {
	edge := &fakeSeenEdge{}
	a := newSeenActivities(t, edge)
	ids := make([]string, 0, 1001)
	for i := 0; i < 1001; i++ {
		ids = append(ids, fmt.Sprintf("frontier-%d", i))
	}
	marked, err := a.FrontierMarkSeen(context.Background(), FrontierMarkSeenInput{Queue: "crawl-q", OrgID: "org_a", RequestIDs: ids})
	require.NoError(t, err)
	require.Equal(t, uint32(1001), marked)
	require.Len(t, edge.sent, 3)
	require.Len(t, edge.sent[0], 500)
	require.Len(t, edge.sent[2], 1)
	require.Equal(t, "/v1/internal/queues/crawl-q/seen", edge.paths[0])
}

func TestFrontierMarkSeen_SkipsBlankIDsAndSendsNothingWhenNoneRemain(t *testing.T) {
	edge := &fakeSeenEdge{}
	a := newSeenActivities(t, edge)
	marked, err := a.FrontierMarkSeen(context.Background(), FrontierMarkSeenInput{Queue: "crawl-q", OrgID: "org_a", RequestIDs: []string{"", "  "}})
	require.NoError(t, err)
	require.Equal(t, uint32(0), marked)
	require.Empty(t, edge.sent)
}
