package boamp

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/eforms"
	"github.com/triodelab/anbud/internal/sources/doffin"
)

var fixedNow = time.Date(2026, 10, 29, 12, 0, 0, 0, time.UTC)

// newTestClient serves h as the records endpoint. The pace is the fastest the client allows, so a test
// with two calls waits one second.
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

func TestSearchAnswersOnePageOfADay(t *testing.T) {
	t.Parallel()
	var got url.Values
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		got = r.URL.Query()
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"total_count": 3, "results": [` +
			`{"idweb": "26-104530", "dateparution": "2026-10-29", "famille": "JOUE"},` +
			`{"idweb": "26-104549", "dateparution": "2026-10-29", "famille": "JOUE"}]}`))
	})
	page := 2
	resp, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(29), IssueDateTo: day(29), NumHitsPerPage: 2, Page: &page})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if got.Get("where") != "dateparution = date'2026-10-29'" || got.Get("order_by") != "idweb" {
		t.Errorf("query where=%q order_by=%q; want the day's records in identifier order", got.Get("where"), got.Get("order_by"))
	}
	if got.Get("limit") != "2" || got.Get("offset") != "2" {
		t.Errorf("paging limit=%q offset=%q; want limit 2 and the second page at offset 2", got.Get("limit"), got.Get("offset"))
	}
	if resp.NumHitsTotal != 3 || len(resp.Hits) != 2 {
		t.Fatalf("total %d, %d hits; want 3 and 2", resp.NumHitsTotal, len(resp.Hits))
	}
	if resp.Hits[0].ID != "26-104530" || resp.Hits[0].PublicationDate != "2026-10-29" {
		t.Errorf("first hit = %+v", resp.Hits[0])
	}
	if !json.Valid(resp.Raw) || resp.ContentType != "application/json" || !resp.FetchedAt.Equal(fixedNow) {
		t.Errorf("evidence not kept: raw valid %v, content type %q, fetched %v", json.Valid(resp.Raw), resp.ContentType, resp.FetchedAt)
	}
}

// TestSearchRefusesWhatTheSourceDoesNotAnswer: a window of more than one day, a page out of range and
// a query past the portal's record window are refused before any request.
func TestSearchRefusesWhatTheSourceDoesNotAnswer(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		t.Errorf("a refused search reached the portal: %s", r.URL)
	})
	one := 1
	past := 101
	for name, p := range map[string]doffin.SearchParams{
		"two days":        {IssueDateFrom: day(28), IssueDateTo: day(29), NumHitsPerPage: 10, Page: &one},
		"no day":          {NumHitsPerPage: 10, Page: &one},
		"page zero":       {IssueDateFrom: day(29), IssueDateTo: day(29), NumHitsPerPage: 10, Page: new(int)},
		"past the window": {IssueDateFrom: day(29), IssueDateTo: day(29), NumHitsPerPage: 100, Page: &past},
	} {
		if _, err := c.Search(context.Background(), p); !errors.Is(err, doffin.ErrInvalidParams) {
			t.Errorf("%s: err = %v, want ErrInvalidParams", name, err)
		}
	}
}

func TestSearchMapsTheSourcesErrors(t *testing.T) {
	t.Parallel()
	page := 1
	params := doffin.SearchParams{IssueDateFrom: day(29), IssueDateTo: day(29), NumHitsPerPage: 10, Page: &page}

	limited := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Retry-After", "7")
		w.WriteHeader(http.StatusTooManyRequests)
	})
	_, err := limited.Search(context.Background(), params)
	var rl *doffin.RateLimitedError
	if !errors.As(err, &rl) || rl.RetryAfter != 7*time.Second {
		t.Errorf("429: err = %v, want a RateLimitedError that waits 7 seconds", err)
	}

	failing := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusInternalServerError)
		_, _ = w.Write([]byte("the portal is down\n"))
	})
	_, err = failing.Search(context.Background(), params)
	var httpErr *doffin.HTTPError
	if !errors.As(err, &httpErr) || httpErr.Status != http.StatusInternalServerError {
		t.Errorf("500: err = %v, want an HTTPError with status 500", err)
	}

	garbled := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`<html>not json</html>`))
	})
	_, err = garbled.Search(context.Background(), params)
	var decode *doffin.DecodeError
	if !errors.As(err, &decode) {
		t.Errorf("a non-JSON answer: err = %v, want a DecodeError", err)
	}
}

func TestDownloadKeepsTheEFormsNotice(t *testing.T) {
	t.Parallel()
	r := loadFixture(t)["26-104530"]
	var got url.Values
	c := newTestClient(t, func(w http.ResponseWriter, req *http.Request) {
		got = req.URL.Query()
		_ = json.NewEncoder(w).Encode(map[string]any{"total_count": 1, "results": []record{{Idweb: r.Idweb, Donnees: r.Donnees}}})
	})
	n, err := c.Download(context.Background(), "26-104530")
	if err != nil {
		t.Fatalf("Download: %v", err)
	}
	if got.Get("where") != `idweb="26-104530"` || got.Get("select") != "idweb,donnees" {
		t.Errorf("query where=%q select=%q", got.Get("where"), got.Get("select"))
	}
	if n.ContentType != "application/xml" || n.ID != "26-104530" {
		t.Fatalf("notice id %q, content type %q; want application/xml for 26-104530", n.ID, n.ContentType)
	}
	sum := sha256.Sum256(n.Body)
	if n.SHA256 != hex.EncodeToString(sum[:]) {
		t.Error("the hash is not the hash of the body")
	}
	if _, err := eforms.Parse(n.Body); err != nil {
		t.Fatalf("the kept body does not read as an eForms notice: %v", err)
	}
}

func TestDownloadKeepsANationalFormAsItsJSON(t *testing.T) {
	t.Parallel()
	r := loadFixture(t)["26-105149"]
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{"total_count": 1, "results": []record{{Idweb: r.Idweb, Donnees: r.Donnees}}})
	})
	n, err := c.Download(context.Background(), "26-105149")
	if err != nil {
		t.Fatalf("Download: %v", err)
	}
	if n.ContentType != "application/json" || string(n.Body) != r.Donnees {
		t.Errorf("content type %q; want the record's JSON, unchanged", n.ContentType)
	}
}

func TestDownloadRefusesWhatIsNotThere(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(`{"total_count": 0, "results": []}`))
	})
	if _, err := c.Download(context.Background(), "26-104530"); !errors.Is(err, doffin.ErrNotFound) {
		t.Errorf("an empty answer: err = %v, want ErrNotFound", err)
	}

	// A Doffin id is not a BOAMP identifier: refused before the portal is asked.
	refusing := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Errorf("a download of a non-BOAMP id reached the portal")
	})
	if _, err := refusing.Download(context.Background(), "2026-116564"); !errors.Is(err, doffin.ErrInvalidParams) {
		t.Errorf("a Doffin id: err = %v, want ErrInvalidParams", err)
	}
}

func TestNewRefusesAnInsecureURLAndAnOutOfRangePace(t *testing.T) {
	t.Parallel()
	if _, err := New(Config{RecordsURL: "http://boamp.example.com/records"}); !errors.Is(err, ErrInvalidConfig) {
		t.Errorf("an http URL off loopback: err = %v, want ErrInvalidConfig", err)
	}
	if _, err := New(Config{SearchRPM: MaxRPM + 1}); !errors.Is(err, ErrInvalidConfig) {
		t.Errorf("a pace above the maximum: err = %v, want ErrInvalidConfig", err)
	}
}

func TestPublicationDatesReadTheSearchEvidence(t *testing.T) {
	t.Parallel()
	body := []byte(`{"total_count": 2, "results": [` +
		`{"idweb": "26-104530", "dateparution": "2026-10-29"},` +
		`{"idweb": "26-104549", "dateparution": "not a date"}]}`)
	dates := PublicationDates(body)
	if dates["26-104530"] != "2026-10-29" || len(dates) != 1 {
		t.Errorf("dates = %v; want only the record with a usable day", dates)
	}
	if got := PublicationDates([]byte(`not json`)); len(got) != 0 {
		t.Errorf("a garbled page gave dates %v", got)
	}
}
