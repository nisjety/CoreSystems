package doffin

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"golang.org/x/time/rate"

	"github.com/triodelab/anbud/internal/secret"
)

const testKey = "0123456789abcdef0123456789abcdef"

// sleepRecorder is a fake clock. It replaces the client's sleep and now, so 429
// back-off is observable and the gate arithmetic is exact without the test
// actually waiting.
type sleepRecorder struct {
	mu        sync.Mutex
	current   time.Time
	durations []time.Duration
}

func newSleepRecorder() *sleepRecorder {
	return &sleepRecorder{current: time.Date(2026, 10, 9, 12, 0, 0, 0, time.UTC)}
}

func (s *sleepRecorder) Now() time.Time {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.current
}

func (s *sleepRecorder) advance(d time.Duration) {
	s.mu.Lock()
	s.current = s.current.Add(d)
	s.mu.Unlock()
}

// sleep records the wait and moves the fake clock forward by it.
func (s *sleepRecorder) sleep(ctx context.Context, d time.Duration) error {
	s.mu.Lock()
	s.durations = append(s.durations, d)
	s.current = s.current.Add(d)
	s.mu.Unlock()
	return ctx.Err()
}

func (s *sleepRecorder) total() time.Duration {
	s.mu.Lock()
	defer s.mu.Unlock()
	var sum time.Duration
	for _, d := range s.durations {
		sum += d
	}
	return sum
}

// newTestClient wires a client at a loopback test server with pacing disabled.
func newTestClient(t *testing.T, h http.Handler, mutate ...func(*Config)) (*Client, *httptest.Server, *sleepRecorder) {
	t.Helper()
	srv := httptest.NewServer(h)
	t.Cleanup(srv.Close)

	cfg := Config{BaseURL: srv.URL + "/public/v2", APIKey: secret.Value(testKey)}
	for _, m := range mutate {
		m(&cfg)
	}
	c, err := New(cfg)
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	c.limiters[opSearch] = rate.NewLimiter(rate.Inf, 1)
	c.limiters[opDownload] = rate.NewLimiter(rate.Inf, 1)
	rec := newSleepRecorder()
	c.sleep = rec.sleep
	c.now = rec.Now
	return c, srv, rec
}

func jsonHandler(body string) http.HandlerFunc {
	return func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(body))
	}
}

func intPtr(n int) *int       { return &n }
func int64Ptr(n int64) *int64 { return &n }

// ---------------------------------------------------------------- New ----

func TestNewRequiresAKey(t *testing.T) {
	t.Parallel()

	_, err := New(Config{})
	if !errors.Is(err, ErrNotConfigured) {
		t.Fatalf("New(no key) error = %v, want ErrNotConfigured", err)
	}
}

func TestNewValidatesTheKeyShapeWithoutEchoingIt(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name string
		key  string
	}{
		{"contains a space", "0123456789 abcdef0123456789abcdef"},
		{"contains a newline", "0123456789abcdef\n0123456789abcdef"},
		{"too short", "abc123"},
		{"non-ascii", "0123456789abcdef0123456789abcdé"},
		{"control character", "0123456789abcdef0123456789abc\x01ef"},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			_, err := New(Config{APIKey: secret.Value(tc.key)})
			if !errors.Is(err, ErrInvalidConfig) {
				t.Fatalf("error = %v, want ErrInvalidConfig", err)
			}
			if strings.Contains(err.Error(), tc.key) {
				t.Fatalf("error echoed the key: %q", err)
			}
		})
	}
}

func TestNewValidatesTheBaseURL(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name     string
		baseURL  string
		wantErr  bool
		wantPath string
	}{
		{"default", "", false, "/public/v2"},
		{"https with trailing slash", "https://example.com/public/v2/", false, "/public/v2"},
		{"http loopback ip", "http://127.0.0.1:8080/x", false, "/x"},
		{"http localhost", "http://localhost:9/x", false, "/x"},
		{"http ipv6 loopback", "http://[::1]:80/x", false, "/x"},
		{"http to a public host would leak the key", "http://api.example.com/x", true, ""},
		{"userinfo", "https://user:pw@example.com/x", true, ""},
		{"query string", "https://example.com/x?a=1", true, ""},
		{"fragment", "https://example.com/x#frag", true, ""},
		{"unsupported scheme", "ftp://example.com/x", true, ""},
		{"no host", "https:///x", true, ""},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			c, err := New(Config{BaseURL: tc.baseURL, APIKey: secret.Value(testKey)})
			if tc.wantErr {
				if !errors.Is(err, ErrInvalidConfig) {
					t.Fatalf("error = %v, want ErrInvalidConfig", err)
				}
				return
			}
			if err != nil {
				t.Fatalf("New: %v", err)
			}
			if c.base.Path != tc.wantPath {
				t.Fatalf("base path = %q, want %q", c.base.Path, tc.wantPath)
			}
		})
	}
}

func TestNewBoundsTheRequestBudgets(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name         string
		search       int
		download     int
		wantErr      bool
		wantSearch   int
		wantDownload int
	}{
		{"defaults", 0, 0, false, 24, 8},
		{"exactly the documented limits", 30, 10, false, 30, 10},
		{"search over the documented limit", 31, 8, true, 0, 0},
		{"download over the documented limit", 24, 11, true, 0, 0},
		{"negative search", -1, 0, true, 0, 0},
		{"negative download", 0, -1, true, 0, 0},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			c, err := New(Config{APIKey: secret.Value(testKey), SearchRPM: tc.search, DownloadRPM: tc.download})
			if tc.wantErr {
				if !errors.Is(err, ErrInvalidConfig) {
					t.Fatalf("error = %v, want ErrInvalidConfig", err)
				}
				return
			}
			if err != nil {
				t.Fatalf("New: %v", err)
			}
			got := c.Budgets()
			if got.SearchRPM != tc.wantSearch || got.DownloadRPM != tc.wantDownload {
				t.Fatalf("Budgets() = %+v, want %d/%d", got, tc.wantSearch, tc.wantDownload)
			}
		})
	}
}

func TestClientNeverRendersTheKey(t *testing.T) {
	t.Parallel()

	c, err := New(Config{APIKey: secret.Value(testKey)})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	rendered := fmt.Sprintf("%v | %+v | %#v | %s", c, c, c, c)
	if strings.Contains(rendered, testKey) {
		t.Fatalf("client rendering leaked the key: %s", rendered)
	}
}

// -------------------------------------------------------------- Search ----

func TestSearchSendsTheKeyAndEncodedParameters(t *testing.T) {
	t.Parallel()

	var (
		mu       sync.Mutex
		gotPath  string
		gotKey   string
		gotAgent string
		gotQuery url.Values
	)
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		gotPath, gotKey, gotAgent, gotQuery = r.URL.Path, r.Header.Get(KeyHeader), r.Header.Get("User-Agent"), r.URL.Query()
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"numHitsTotal":0,"numHitsAccessible":0,"hits":[]}`))
	}))

	_, err := c.Search(context.Background(), SearchParams{
		SearchString:       "vask og rengjøring",
		Types:              []string{"COMPETITION", "RESULT"},
		Statuses:           []string{"ACTIVE", "AWARDED"},
		CPVCodes:           []string{"90910000", "39830000"},
		Locations:          []string{"NO081", "anyw"},
		SortBy:             "PUBLICATION_DATE_DESC",
		Page:               intPtr(2),
		NumHitsPerPage:     50,
		IssueDateFrom:      time.Date(2026, 1, 2, 0, 0, 0, 0, time.UTC),
		IssueDateTo:        time.Date(2026, 2, 3, 0, 0, 0, 0, time.UTC),
		EstimatedValueFrom: int64Ptr(1_000_000),
		EstimatedValueTo:   int64Ptr(5_000_000),
	})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}

	mu.Lock()
	defer mu.Unlock()
	if gotPath != "/public/v2/search" {
		t.Errorf("path = %q", gotPath)
	}
	if gotKey != testKey {
		t.Errorf("subscription key header not sent")
	}
	if gotAgent == "" || strings.Contains(gotAgent, testKey) {
		t.Errorf("User-Agent = %q", gotAgent)
	}
	want := url.Values{
		"searchString":       {"vask og rengjøring"},
		"type":               {"COMPETITION", "RESULT"},
		"status":             {"ACTIVE", "AWARDED"},
		"cpvCode":            {"90910000", "39830000"},
		"location":           {"NO081", "anyw"},
		"sortBy":             {"PUBLICATION_DATE_DESC"},
		"page":               {"2"},
		"numHitsPerPage":     {"50"},
		"issueDateFrom":      {"2026-01-02"},
		"issueDateTo":        {"2026-02-03"},
		"estimatedValueFrom": {"1000000"},
		"estimatedValueTo":   {"5000000"},
	}
	for key, wantValues := range want {
		if got := gotQuery[key]; strings.Join(got, "|") != strings.Join(wantValues, "|") {
			t.Errorf("query %s = %v, want %v", key, got, wantValues)
		}
	}
	if len(gotQuery) != len(want) {
		t.Errorf("unexpected query parameters: %v", gotQuery)
	}
}

func TestSearchOmitsUnsetParametersAndAllowsPageZero(t *testing.T) {
	t.Parallel()

	var rawQuery atomic.Value
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		rawQuery.Store(r.URL.RawQuery)
		_, _ = w.Write([]byte(`{"hits":[]}`))
	}))

	if _, err := c.Search(context.Background(), SearchParams{}); err != nil {
		t.Fatalf("Search: %v", err)
	}
	if got := rawQuery.Load().(string); got != "" {
		t.Errorf("empty params produced query %q", got)
	}

	// An explicit page 0 is sent as given: the client does not drop it (the API itself
	// refuses it, since paging is 1-based).
	if _, err := c.Search(context.Background(), SearchParams{Page: intPtr(0)}); err != nil {
		t.Fatalf("Search: %v", err)
	}
	if got := rawQuery.Load().(string); got != "page=0" {
		t.Errorf("page=0 produced query %q", got)
	}
}

func TestSearchRejectsInvalidParametersWithoutCallingTheAPI(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name   string
		params SearchParams
	}{
		{"page size above the documented maximum", SearchParams{NumHitsPerPage: 101}},
		{"negative page size", SearchParams{NumHitsPerPage: -1}},
		{"negative page", SearchParams{Page: intPtr(-1)}},
		{"type with an injected parameter", SearchParams{Types: []string{"COMPETITION&x=1"}}},
		{"lowercase type", SearchParams{Types: []string{"competition"}}},
		{"empty type", SearchParams{Types: []string{""}}},
		{"status with punctuation", SearchParams{Statuses: []string{"ACTIVE;DROP"}}},
		{"non numeric cpv", SearchParams{CPVCodes: []string{"abc"}}},
		{"overlong cpv", SearchParams{CPVCodes: []string{"1234567890123"}}},
		{"location with a space", SearchParams{Locations: []string{"a b"}}},
		{"location with traversal", SearchParams{Locations: []string{"../x"}}},
		{"sort with a space", SearchParams{SortBy: "bad value"}},
		{"search text with a control character", SearchParams{SearchString: "a\x00b"}},
		{"search text too long", SearchParams{SearchString: strings.Repeat("a", 201)}},
		{"issue date range reversed", SearchParams{
			IssueDateFrom: time.Date(2026, 2, 1, 0, 0, 0, 0, time.UTC),
			IssueDateTo:   time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC),
		}},
		{"value range reversed", SearchParams{EstimatedValueFrom: int64Ptr(10), EstimatedValueTo: int64Ptr(5)}},
		{"negative value", SearchParams{EstimatedValueFrom: int64Ptr(-1)}},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			var hits atomic.Int32
			c, _, _ := newTestClient(t, http.HandlerFunc(func(http.ResponseWriter, *http.Request) { hits.Add(1) }))

			_, err := c.Search(context.Background(), tc.params)
			if !errors.Is(err, ErrInvalidParams) {
				t.Fatalf("error = %v, want ErrInvalidParams", err)
			}
			if hits.Load() != 0 {
				t.Fatalf("invalid params still reached the API (%d calls)", hits.Load())
			}
		})
	}
}

const sampleSearchBody = `{
  "numHitsTotal": 1234,
  "numHitsAccessible": 1000,
  "hits": [{
    "id": "2025-104837",
    "buyer": [{"id": "b1", "organizationId": "974760673", "name": "Statens vegvesen"}],
    "heading": "Rammeavtale vask",
    "description": "Vask og rengjøring av kontorlokaler",
    "locationId": ["NO081"],
    "estimatedValue": {"currencyCode": "NOK", "amount": 1500000.5},
    "type": "ANNOUNCEMENT_OF_COMPETITION",
    "allTypes": ["ANNOUNCEMENT_OF_COMPETITION"],
    "status": "ACTIVE",
    "issueDate": "2025-12-01",
    "deadline": "2026-01-15T11:00:00Z",
    "publicationDate": "2025-12-01",
    "receivedTenders": 0,
    "allReceivedTenders": [{"type": "all", "total": 3}],
    "cpvCodes": ["90910000"],
    "limitedDataFlag": false,
    "doffinClassicUrl": "https://doffin.no/Notice/Details/2025-104837",
    "lots": [{"heading": "Lot 1", "description": "Kontor", "winner": [{"id": "w1", "organizationId": "123456789", "name": "Vask AS"}]}],
    "aFieldDoffinAddsLater": {"x": 1}
  }]
}`

func TestSearchDecodesTheResponseAndKeepsTheRawBody(t *testing.T) {
	t.Parallel()

	c, _, _ := newTestClient(t, jsonHandler(sampleSearchBody))
	resp, err := c.Search(context.Background(), SearchParams{})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}

	if resp.NumHitsTotal != 1234 || resp.NumHitsAccessible != 1000 {
		t.Errorf("totals = %d/%d", resp.NumHitsTotal, resp.NumHitsAccessible)
	}
	if len(resp.Hits) != 1 {
		t.Fatalf("hits = %d, want 1", len(resp.Hits))
	}
	hit := resp.Hits[0]
	if hit.ID != "2025-104837" || hit.Type != "ANNOUNCEMENT_OF_COMPETITION" {
		t.Errorf("hit = %+v", hit)
	}
	if len(hit.Buyer) != 1 || hit.Buyer[0].OrganizationID != "974760673" || hit.Buyer[0].Name != "Statens vegvesen" {
		t.Errorf("buyer = %+v", hit.Buyer)
	}
	if hit.EstimatedValue == nil || hit.EstimatedValue.CurrencyCode != "NOK" || hit.EstimatedValue.Amount.String() != "1500000.5" {
		t.Errorf("estimatedValue = %+v", hit.EstimatedValue)
	}
	if len(hit.CPVCodes) != 1 || hit.CPVCodes[0] != "90910000" {
		t.Errorf("cpv = %v", hit.CPVCodes)
	}
	if len(hit.Lots) != 1 || len(hit.Lots[0].Winner) != 1 || hit.Lots[0].Winner[0].Name != "Vask AS" {
		t.Errorf("lots = %+v", hit.Lots)
	}
	if len(hit.AllReceivedTenders) != 1 || hit.AllReceivedTenders[0].Total != 3 {
		t.Errorf("allReceivedTenders = %+v", hit.AllReceivedTenders)
	}
	// Live captures show status and deadline are a string or null.
	if hit.Status == nil || *hit.Status != "ACTIVE" || hit.Deadline == nil || *hit.Deadline != "2026-01-15T11:00:00Z" {
		t.Errorf("status/deadline = %v / %v", hit.Status, hit.Deadline)
	}
	if string(resp.Raw) != sampleSearchBody {
		t.Errorf("raw body was not preserved")
	}
}

func TestSearchToleratesHitsThatAreNotAnArray(t *testing.T) {
	t.Parallel()

	// The published OpenAPI types `hits` as a single object although the
	// response is a list; stay correct whichever the live API really sends.
	tests := []struct {
		name     string
		body     string
		wantHits int
	}{
		{"array", `{"numHitsTotal":2,"hits":[{"id":"2025-1"},{"id":"2025-2"}]}`, 2},
		{"single object", `{"numHitsTotal":1,"hits":{"id":"2025-1"}}`, 1},
		{"null", `{"numHitsTotal":0,"hits":null}`, 0},
		{"absent", `{"numHitsTotal":0}`, 0},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			c, _, _ := newTestClient(t, jsonHandler(tc.body))
			resp, err := c.Search(context.Background(), SearchParams{})
			if err != nil {
				t.Fatalf("Search: %v", err)
			}
			if len(resp.Hits) != tc.wantHits {
				t.Fatalf("hits = %d, want %d", len(resp.Hits), tc.wantHits)
			}
		})
	}
}

func TestSearchRejectsABodyThatIsNotJSON(t *testing.T) {
	t.Parallel()

	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte("<html>maintenance</html>"))
	}))
	if _, err := c.Search(context.Background(), SearchParams{}); err == nil {
		t.Fatal("expected a decode error")
	}
}

func TestSearchCapsTheResponseSize(t *testing.T) {
	t.Parallel()

	c, _, _ := newTestClient(t, jsonHandler(`{"hits":[]}`+strings.Repeat(" ", 64)), func(cfg *Config) { cfg.MaxSearchBytes = 32 })
	_, err := c.Search(context.Background(), SearchParams{})
	if !errors.Is(err, ErrResponseTooLarge) {
		t.Fatalf("error = %v, want ErrResponseTooLarge", err)
	}
}

func TestSearchMapsHTTPErrors(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name        string
		status      int
		body        string
		wantIs      error
		wantMessage string
	}{
		{"unauthorized", 401, `{"statusCode":401,"message":"Access denied due to invalid subscription key."}`, ErrUnauthorized, "invalid subscription key"},
		{"forbidden", 403, `{"message":"nope"}`, ErrUnauthorized, "nope"},
		{"bad request", 400, `{"message":"Invalid sortBy"}`, nil, "Invalid sortBy"},
		{"server error", 500, `internal`, nil, ""},
		{"gateway html", 502, `<html>bad gateway</html>`, nil, ""},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(tc.status)
				_, _ = w.Write([]byte(tc.body))
			}))
			_, err := c.Search(context.Background(), SearchParams{})
			var httpErr *HTTPError
			if !errors.As(err, &httpErr) {
				t.Fatalf("error = %v, want *HTTPError", err)
			}
			if httpErr.Status != tc.status {
				t.Errorf("status = %d, want %d", httpErr.Status, tc.status)
			}
			if tc.wantIs != nil && !errors.Is(err, tc.wantIs) {
				t.Errorf("error %v should match %v", err, tc.wantIs)
			}
			if tc.wantMessage != "" && !strings.Contains(httpErr.Message, tc.wantMessage) {
				t.Errorf("message = %q, want it to contain %q", httpErr.Message, tc.wantMessage)
			}
		})
	}
}

func TestHTTPErrorsAreSanitisedAndNeverEchoTheKey(t *testing.T) {
	t.Parallel()

	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(400)
		// A hostile or buggy upstream echoing the key, control characters and a novel.
		_, _ = fmt.Fprintf(w, `{"message":"bad key %s \u0000\u001b[31m %s"}`, testKey, strings.Repeat("x", 5000))
	}))
	_, err := c.Search(context.Background(), SearchParams{})
	if err == nil {
		t.Fatal("expected an error")
	}
	if strings.Contains(err.Error(), testKey) {
		t.Fatalf("error leaked the key: %q", err)
	}
	if strings.ContainsAny(err.Error(), "\x00\x1b") {
		t.Fatalf("error carries control characters: %q", err)
	}
	if len(err.Error()) > 600 {
		t.Fatalf("error not truncated (%d bytes)", len(err.Error()))
	}
}

func TestSearchRetriesAfterA429(t *testing.T) {
	t.Parallel()

	var calls atomic.Int32
	c, _, sleeps := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		if calls.Add(1) == 1 {
			w.Header().Set("Retry-After", "7")
			w.WriteHeader(http.StatusTooManyRequests)
			return
		}
		_, _ = w.Write([]byte(`{"numHitsTotal":0,"hits":[]}`))
	}))

	if _, err := c.Search(context.Background(), SearchParams{}); err != nil {
		t.Fatalf("Search: %v", err)
	}
	if calls.Load() != 2 {
		t.Fatalf("calls = %d, want 2", calls.Load())
	}
	if got := sleeps.total(); got != 7*time.Second {
		t.Fatalf("waited %v, want exactly the 7s Retry-After", got)
	}
}

func TestSearchDoesNotWaitOutAnUnreasonableRetryAfter(t *testing.T) {
	t.Parallel()

	var calls atomic.Int32
	c, _, sleeps := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		if calls.Add(1) == 1 {
			w.Header().Set("Retry-After", "600")
			w.WriteHeader(http.StatusTooManyRequests)
			return
		}
		_, _ = w.Write([]byte(`{"hits":[]}`))
	}))

	_, err := c.Search(context.Background(), SearchParams{})
	var limited *RateLimitedError
	if !errors.As(err, &limited) {
		t.Fatalf("error = %v, want *RateLimitedError", err)
	}
	if limited.RetryAfter != 10*time.Minute {
		t.Errorf("RetryAfter = %v, want 10m", limited.RetryAfter)
	}
	if calls.Load() != 1 {
		t.Errorf("calls = %d, want 1 (no retry within the call)", calls.Load())
	}
	if sleeps.total() != 0 {
		t.Errorf("slept %v inside the failed call", sleeps.total())
	}

	// The 429 closes a gate shared by every caller. A later call must neither
	// hammer the limited endpoint nor sleep for ten minutes: it is told, at
	// once, how long to stay away.
	_, err = c.Search(context.Background(), SearchParams{})
	if !errors.As(err, &limited) || limited.RetryAfter != 10*time.Minute {
		t.Fatalf("second call error = %v, want *RateLimitedError(10m)", err)
	}
	if calls.Load() != 1 {
		t.Errorf("the second call reached the API (calls = %d)", calls.Load())
	}
	if sleeps.total() != 0 {
		t.Errorf("the second call slept %v instead of failing fast", sleeps.total())
	}

	// Once the advertised window has passed, calls go through again.
	sleeps.advance(10 * time.Minute)
	if _, err := c.Search(context.Background(), SearchParams{}); err != nil {
		t.Fatalf("Search after the window: %v", err)
	}
}

func TestSearchGivesUpAfterTheRetryBudget(t *testing.T) {
	t.Parallel()

	var calls atomic.Int32
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		w.Header().Set("Retry-After", "1")
		w.WriteHeader(http.StatusTooManyRequests)
	}))

	_, err := c.Search(context.Background(), SearchParams{})
	var limited *RateLimitedError
	if !errors.As(err, &limited) {
		t.Fatalf("error = %v, want *RateLimitedError", err)
	}
	if calls.Load() != 3 { // the first attempt plus the default of two retries
		t.Fatalf("calls = %d, want 3", calls.Load())
	}
}

// The limiter refuses up front when the wait would outlast the deadline; this
// pins the mapping of that refusal. Cancellation while genuinely blocked is
// covered in client_hardening_test.go.
func TestSearchFailsFastWhenTheDeadlineCannotBeMet(t *testing.T) {
	t.Parallel()

	var calls atomic.Int32
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		_, _ = w.Write([]byte(`{"hits":[]}`))
	}))
	// One request per hour with a burst of one: the first call spends the token.
	c.limiters[opSearch] = rate.NewLimiter(rate.Every(time.Hour), 1)

	if _, err := c.Search(context.Background(), SearchParams{}); err != nil {
		t.Fatalf("first Search: %v", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	_, err := c.Search(ctx, SearchParams{})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("error = %v, want context.DeadlineExceeded", err)
	}
	if calls.Load() != 1 {
		t.Fatalf("the second call must not reach the API (calls = %d)", calls.Load())
	}
}

func TestSearchAndDownloadHaveIndependentBudgets(t *testing.T) {
	t.Parallel()

	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.Contains(r.URL.Path, "/download/") {
			_, _ = w.Write([]byte("<notice/>"))
			return
		}
		_, _ = w.Write([]byte(`{"hits":[]}`))
	}))
	c.limiters[opSearch] = rate.NewLimiter(rate.Every(time.Hour), 1)
	c.limiters[opDownload] = rate.NewLimiter(rate.Every(time.Hour), 1)

	if _, err := c.Search(context.Background(), SearchParams{}); err != nil {
		t.Fatalf("Search: %v", err)
	}
	// Spending the search token must not starve download.
	if _, err := c.Download(context.Background(), "2025-104837"); err != nil {
		t.Fatalf("Download: %v", err)
	}
}

// Proves race-cleanliness under -race only; the 429 gate semantics under
// concurrency are asserted in client_hardening_test.go.
func TestSearchIsRaceFreeUnderConcurrentUse(t *testing.T) {
	t.Parallel()

	var calls atomic.Int32
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		if calls.Add(1)%4 == 0 {
			w.Header().Set("Retry-After", "1")
			w.WriteHeader(http.StatusTooManyRequests)
			return
		}
		_, _ = w.Write([]byte(`{"numHitsTotal":1,"hits":[{"id":"2025-1"}]}`))
	}))

	var wg sync.WaitGroup
	for range 16 {
		wg.Go(func() {
			if _, err := c.Search(context.Background(), SearchParams{}); err != nil {
				var limited *RateLimitedError
				if !errors.As(err, &limited) {
					t.Errorf("Search: %v", err)
				}
			}
		})
	}
	wg.Wait()
}

func TestParseRetryAfter(t *testing.T) {
	t.Parallel()

	now := time.Date(2026, 10, 9, 12, 0, 0, 0, time.UTC)
	const fallback = 60 * time.Second
	tests := []struct {
		name   string
		header string
		want   time.Duration
	}{
		{"delta seconds", "7", 7 * time.Second},
		{"zero", "0", 0},
		{"negative is clamped", "-5", 0},
		{"missing uses the fallback", "", fallback},
		{"garbage uses the fallback", "soon", fallback},
		{"fractional is invalid", "1.5", fallback},
		{"http date in the future", "Fri, 09 Oct 2026 12:00:30 GMT", 30 * time.Second},
		{"http date in the past", "Fri, 09 Oct 2026 11:59:00 GMT", 0},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			if got := parseRetryAfter(tc.header, now, fallback); got != tc.want {
				t.Fatalf("parseRetryAfter(%q) = %v, want %v", tc.header, got, tc.want)
			}
		})
	}
}

func TestClientDoesNotFollowRedirectsSoTheKeyCannotLeak(t *testing.T) {
	t.Parallel()

	var leaked atomic.Int32
	other := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get(KeyHeader) != "" {
			leaked.Add(1)
		}
		_, _ = w.Write([]byte(`{"hits":[]}`))
	}))
	t.Cleanup(other.Close)

	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, other.URL+"/steal", http.StatusFound)
	}))

	_, err := c.Search(context.Background(), SearchParams{})
	var httpErr *HTTPError
	if !errors.As(err, &httpErr) || httpErr.Status != http.StatusFound {
		t.Fatalf("error = %v, want *HTTPError with status 302", err)
	}
	if leaked.Load() != 0 {
		t.Fatal("the subscription key was forwarded to a redirect target")
	}
}

// ------------------------------------------------------------ Download ----

func TestDownloadReturnsTheRawNoticeWithItsHash(t *testing.T) {
	t.Parallel()

	const body = `<?xml version="1.0"?><ContractNotice/>`
	var gotPath, gotKey string
	var mu sync.Mutex
	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		gotPath, gotKey = r.URL.Path, r.Header.Get(KeyHeader)
		mu.Unlock()
		w.Header().Set("Content-Type", "application/xml")
		_, _ = w.Write([]byte(body))
	}))
	fixed := time.Date(2026, 10, 9, 12, 0, 0, 0, time.UTC)
	c.now = func() time.Time { return fixed }

	n, err := c.Download(context.Background(), "2025-104837")
	if err != nil {
		t.Fatalf("Download: %v", err)
	}

	mu.Lock()
	defer mu.Unlock()
	if gotPath != "/public/v2/download/2025-104837" {
		t.Errorf("path = %q", gotPath)
	}
	if gotKey != testKey {
		t.Error("subscription key header not sent")
	}
	sum := sha256.Sum256([]byte(body))
	if n.ID != "2025-104837" || n.ContentType != "application/xml" || string(n.Body) != body || n.SHA256 != hex.EncodeToString(sum[:]) {
		t.Errorf("notice = %+v", n)
	}
	if !n.FetchedAt.Equal(fixed) {
		t.Errorf("FetchedAt = %v, want %v", n.FetchedAt, fixed)
	}
}

func TestDownloadRejectsMalformedIDsWithoutCallingTheAPI(t *testing.T) {
	t.Parallel()

	for _, id := range []string{
		"", " ", "2025", "2025-", "2025-1", "abcd-123456", "2025-104837/../x", "../etc/passwd",
		"2025-104837?x=1", " 2025-104837", "2025-104837\n", "2025-104837#frag", "2025-1048370000",
	} {
		t.Run(fmt.Sprintf("%q", id), func(t *testing.T) {
			t.Parallel()
			var hits atomic.Int32
			c, _, _ := newTestClient(t, http.HandlerFunc(func(http.ResponseWriter, *http.Request) { hits.Add(1) }))

			_, err := c.Download(context.Background(), id)
			if !errors.Is(err, ErrInvalidParams) {
				t.Fatalf("error = %v, want ErrInvalidParams", err)
			}
			if hits.Load() != 0 {
				t.Fatal("a malformed ID reached the API")
			}
		})
	}
}

func TestDownloadMapsNotFound(t *testing.T) {
	t.Parallel()

	c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
		_, _ = w.Write([]byte(`{"statusCode":404,"message":"Resource not found"}`))
	}))
	_, err := c.Download(context.Background(), "2025-104837")
	if !errors.Is(err, ErrNotFound) {
		t.Fatalf("error = %v, want ErrNotFound", err)
	}
}

func TestDownloadCapsTheSizeAndRefusesAnEmptyBody(t *testing.T) {
	t.Parallel()

	tests := []struct {
		name    string
		body    string
		wantErr error
	}{
		{"exactly at the cap", strings.Repeat("a", 10), nil},
		{"one byte over the cap", strings.Repeat("a", 11), ErrResponseTooLarge},
		{"empty body", "", ErrEmptyResponse},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			c, _, _ := newTestClient(t, http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				_, _ = w.Write([]byte(tc.body))
			}), func(cfg *Config) { cfg.MaxDownloadBytes = 10 })

			_, err := c.Download(context.Background(), "2025-104837")
			if tc.wantErr == nil {
				if err != nil {
					t.Fatalf("Download: %v", err)
				}
				return
			}
			if !errors.Is(err, tc.wantErr) {
				t.Fatalf("error = %v, want %v", err, tc.wantErr)
			}
		})
	}
}
