package bzp

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"strconv"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

var fixedNow = time.Date(2026, 10, 29, 12, 0, 0, 0, time.UTC)

// fixtures reads the three board records of testdata/records_2026-10-28.json, one per kind of notice: a
// contract notice, a tender result and a tender plan.
func fixtures(t *testing.T) []json.RawMessage {
	t.Helper()
	raw, err := os.ReadFile("testdata/records_2026-10-28.json")
	if err != nil {
		t.Fatal(err)
	}
	var recs []json.RawMessage
	if err := json.Unmarshal(raw, &recs); err != nil {
		t.Fatal(err)
	}
	if len(recs) != 3 {
		t.Fatalf("the fixture holds %d records, want 3", len(recs))
	}
	return recs
}

// tedCopy is a notice a Polish buyer published on TED, which the board also lists: the sync leaves it out.
const tedCopy = `{"noticeNumber":"2026/S 209-745248","bzpNumber":"2026/S 209-745248","noticeType":null,"publicationDate":"2026-10-28T00:00:00Z","orderObject":"x","cpvCode":"38000000","organizationName":"x"}`

// newTestClient serves h as the board. The pace is the fastest the client allows.
func newTestClient(t *testing.T, h http.HandlerFunc) *Client {
	t.Helper()
	srv := httptest.NewServer(h)
	t.Cleanup(srv.Close)
	c, err := New(Config{RecordsURL: srv.URL, SearchRPM: MaxRPM, DownloadRPM: MaxRPM, Now: func() time.Time { return fixedNow }})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return c
}

func day(d int) time.Time { return time.Date(2026, 10, d, 0, 0, 0, 0, time.UTC) }

// writeRecords answers a page with the records given, each as its raw JSON.
func writeRecords(w http.ResponseWriter, recs ...json.RawMessage) {
	w.Header().Set("Content-Type", "application/json")
	out := []byte("[")
	for i, r := range recs {
		if i > 0 {
			out = append(out, ',')
		}
		out = append(out, r...)
	}
	_, _ = w.Write(append(out, ']'))
}

// TestSearchReadsTheDayAndLeavesOutWhatIsNotBZPs: the board states no total, so the day is read page by page
// and each notice counted once. A TED copy the board lists is left out, and a pass that finds nothing new
// ends the walk.
func TestSearchReadsTheDayAndLeavesOutWhatIsNotBZPs(t *testing.T) {
	t.Parallel()
	recs := fixtures(t)
	var mu sync.Mutex
	var pages []string
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		q := r.URL.Query()
		if q.Get("publicationDateFrom") != "2026-10-28" || q.Get("publicationDateTo") != "2026-10-28" {
			t.Errorf("a listing request names the wrong day: %s", r.URL)
		}
		mu.Lock()
		pages = append(pages, q.Get("PageNumber"))
		mu.Unlock()
		switch q.Get("PageNumber") {
		case "1":
			writeRecords(w, recs[0], json.RawMessage(tedCopy), recs[0])
		case "2":
			writeRecords(w, recs[1])
		default:
			writeRecords(w)
		}
	})
	page := 1
	resp, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(28), IssueDateTo: day(28), NumHitsPerPage: 10, Page: &page})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if resp.NumHitsTotal != 2 || len(resp.Hits) != 2 {
		t.Fatalf("total %d, hits %+v; want the two BZP notices, each once", resp.NumHitsTotal, resp.Hits)
	}
	if resp.Hits[0].ID != "2026-00505976-01" || resp.Hits[1].ID != "2026-00506555-01" || resp.Hits[0].PublicationDate != "2026-10-28" {
		t.Errorf("hits = %+v", resp.Hits)
	}
	// Pass one reads pages 1 to 3, pass two finds nothing new and reads them again: six listing requests.
	if len(pages) != 6 {
		t.Errorf("the day was read with %d requests, want 6 (two passes of three pages)", len(pages))
	}
	if !json.Valid(resp.Raw) || resp.ContentType != "application/json" {
		t.Errorf("the evidence is not the page's listing JSON: %q", resp.Raw)
	}
}

// TestSearchAnswersTheEnginesPagesFromOneRead: the engine's numbered pages come from the one reading of the
// day, with the same total on each.
func TestSearchAnswersTheEnginesPagesFromOneRead(t *testing.T) {
	t.Parallel()
	recs := fixtures(t)
	var mu sync.Mutex
	reads := 0
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Query().Get("PageNumber") == "1" {
			mu.Lock()
			reads++
			mu.Unlock()
		}
		switch r.URL.Query().Get("PageNumber") {
		case "1":
			writeRecords(w, recs[0], recs[1])
		case "2":
			writeRecords(w, recs[2])
		default:
			writeRecords(w)
		}
	})
	first, second := 1, 2
	p1, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(28), IssueDateTo: day(28), NumHitsPerPage: 2, Page: &first})
	if err != nil {
		t.Fatal(err)
	}
	p2, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(28), IssueDateTo: day(28), NumHitsPerPage: 2, Page: &second})
	if err != nil {
		t.Fatal(err)
	}
	if p1.NumHitsTotal != 3 || p2.NumHitsTotal != 3 || len(p1.Hits) != 2 || len(p2.Hits) != 1 {
		t.Fatalf("pages = total %d/%d, hits %d/%d; want 3, with 2 then 1", p1.NumHitsTotal, p2.NumHitsTotal, len(p1.Hits), len(p2.Hits))
	}
	if reads != 2 {
		t.Errorf("the first page was read %d times, want 2 (one per pass, once per day)", reads)
	}
}

func TestSearchRefusesWhatTheBoardDoesNotAnswer(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Errorf("a refused search reached the board")
	})
	one := 1
	for name, p := range map[string]doffin.SearchParams{
		"two days": {IssueDateFrom: day(27), IssueDateTo: day(28), NumHitsPerPage: 10, Page: &one},
		"no day":   {NumHitsPerPage: 10, Page: &one},
		"page 0":   {IssueDateFrom: day(28), IssueDateTo: day(28), NumHitsPerPage: 10, Page: new(int)},
	} {
		if _, err := c.Search(context.Background(), p); !errors.Is(err, doffin.ErrInvalidParams) {
			t.Errorf("%s: err = %v, want ErrInvalidParams", name, err)
		}
	}
}

// TestDownloadKeepsTheRecordByItsNotice: a notice is fetched by its number, and the record the board gave is
// kept as it came. Both the contract notice's form and the planning notice's, with its /P, are asked for.
func TestDownloadKeepsTheRecordByItsNotice(t *testing.T) {
	t.Parallel()
	recs := fixtures(t)
	byNumber := map[string]json.RawMessage{
		"2026/BZP 00505976/01":   recs[0],
		"2025/BZP 00610043/06/P": recs[2],
	}
	var asked []string
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		number := r.URL.Query().Get("noticeNumber")
		asked = append(asked, number)
		if rec, ok := byNumber[number]; ok {
			writeRecords(w, rec)
			return
		}
		writeRecords(w)
	})
	for _, tc := range []struct {
		id   string
		want json.RawMessage
	}{{"2026-00505976-01", recs[0]}, {"2025-00610043-06-P", recs[2]}} {
		n, err := c.Download(context.Background(), tc.id)
		if err != nil {
			t.Fatalf("Download %s: %v", tc.id, err)
		}
		sum := sha256.Sum256(tc.want)
		if string(n.Body) != string(tc.want) || n.SHA256 != hex.EncodeToString(sum[:]) || n.ContentType != "application/json" {
			t.Errorf("%s: body not the board's record, or wrong hash/content type", tc.id)
		}
	}
	if len(asked) != 2 || asked[0] != "2026/BZP 00505976/01" || asked[1] != "2025/BZP 00610043/06/P" {
		t.Errorf("asked for %q; want the notice numbers in BZP's form", asked)
	}
}

func TestDownloadRefusesWhatIsNotThere(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		writeRecords(w)
	})
	if _, err := c.Download(context.Background(), "2026-00505976-01"); !errors.Is(err, doffin.ErrNotFound) {
		t.Errorf("no record: err = %v, want ErrNotFound", err)
	}
	refusing := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Errorf("a download of a non-BZP id reached the board")
	})
	if _, err := refusing.Download(context.Background(), "2026-116564"); !errors.Is(err, doffin.ErrInvalidParams) {
		t.Errorf("a Doffin id: err = %v, want ErrInvalidParams", err)
	}
}

// TestTransientFailuresAreTriedAgain: a server error is sent again and a success ends the retries; a client
// error is not sent again; a failure that persists stops after maxAttempts tries.
func TestTransientFailuresAreTriedAgain(t *testing.T) {
	t.Parallel()
	recs := fixtures(t)
	page := 1
	params := doffin.SearchParams{IssueDateFrom: day(28), IssueDateTo: day(28), NumHitsPerPage: 10, Page: &page}

	var flaky int
	recovering := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Query().Get("PageNumber") == "1" {
			flaky++
			if flaky == 1 {
				w.WriteHeader(http.StatusServiceUnavailable)
				return
			}
			writeRecords(w, recs[0])
			return
		}
		writeRecords(w)
	})
	recovering.backoff = time.Millisecond
	if _, err := recovering.Search(context.Background(), params); err != nil {
		t.Fatalf("a server error once, then success: err = %v", err)
	}

	var clientErrors int
	notFound := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		clientErrors++
		w.WriteHeader(http.StatusNotFound)
	})
	notFound.backoff = time.Millisecond
	if _, err := notFound.Search(context.Background(), params); !errors.Is(err, doffin.ErrNotFound) || clientErrors != 1 {
		t.Errorf("a 404 was asked for %d times, err %v; want once, ErrNotFound", clientErrors, err)
	}

	var serverErrors int
	broken := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		serverErrors++
		w.WriteHeader(http.StatusBadGateway)
	})
	broken.backoff = time.Millisecond
	var httpErr *doffin.HTTPError
	if _, err := broken.Search(context.Background(), params); !errors.As(err, &httpErr) || serverErrors != maxAttempts {
		t.Errorf("a failure that persists was asked for %d times, err %v; want %d tries", serverErrors, err, maxAttempts)
	}
}

func TestSearchMapsTheBoardsRateLimit(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Retry-After", strconv.Itoa(11))
		w.WriteHeader(http.StatusTooManyRequests)
	})
	page := 1
	_, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(28), IssueDateTo: day(28), NumHitsPerPage: 10, Page: &page})
	var rl *doffin.RateLimitedError
	if !errors.As(err, &rl) || rl.RetryAfter != 11*time.Second {
		t.Errorf("429: err = %v, want a RateLimitedError that waits 11 seconds", err)
	}
}

// TestPublicationDatesReadTheListing: the day a notice was published, read from the stored listing, keyed by
// the notice's public source identifier.
func TestPublicationDatesReadTheListing(t *testing.T) {
	t.Parallel()
	recs := fixtures(t)
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Query().Get("PageNumber") == "1" {
			writeRecords(w, recs[0])
			return
		}
		writeRecords(w)
	})
	page := 1
	resp, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(28), IssueDateTo: day(28), NumHitsPerPage: 10, Page: &page})
	if err != nil {
		t.Fatal(err)
	}
	dates := PublicationDates(resp.Raw)
	if dates["2026-00505976-01"] != "2026-10-28" || len(dates) != 1 {
		t.Errorf("dates = %v, want the notice's day", dates)
	}
	if got := PublicationDates([]byte(`not json`)); len(got) != 0 {
		t.Errorf("a garbled listing gave dates %v", got)
	}
}

// TestNoticeNumbersAndIDsAreReversible: a notice's number in BZP's form and its id carry the same notice, in
// both directions, with the planning suffix kept.
func TestNoticeNumbersAndIDsAreReversible(t *testing.T) {
	t.Parallel()
	for number, id := range map[string]string{
		"2026/BZP 00505976/01":   "2026-00505976-01",
		"2025/BZP 00610043/06/P": "2025-00610043-06-P",
	} {
		if got, ok := idOf(number); !ok || got != id {
			t.Errorf("idOf(%q) = %q, %v; want %q", number, got, ok, id)
		}
		if got, ok := noticeNumberOf(id); !ok || got != number {
			t.Errorf("noticeNumberOf(%q) = %q, %v; want %q", id, got, ok, number)
		}
	}
	for _, bad := range []string{"2026/S 209-745248", "2026/BZP 0050597/01", "26-104530"} {
		if _, ok := idOf(bad); ok {
			t.Errorf("idOf(%q) accepted a number that is not a BZP notice's", bad)
		}
	}
}

func TestNewRefusesAnInsecureURLAndAnOutOfRangePace(t *testing.T) {
	t.Parallel()
	if _, err := New(Config{RecordsURL: "http://bzp.example.com/search"}); !errors.Is(err, ErrInvalidConfig) {
		t.Errorf("an http URL off loopback: err = %v, want ErrInvalidConfig", err)
	}
	if _, err := New(Config{SearchRPM: MaxRPM + 1}); !errors.Is(err, ErrInvalidConfig) {
		t.Errorf("a pace above the maximum: err = %v, want ErrInvalidConfig", err)
	}
}
