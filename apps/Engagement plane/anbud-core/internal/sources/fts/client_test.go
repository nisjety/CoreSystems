package fts

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

var fixedNow = time.Date(2026, 10, 29, 12, 0, 0, 0, time.UTC)

// newTestClient serves h as the service. The pace is the fastest the client allows.
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

// listingPage is one page of a day's listing, with the link to the next page when there is one.
func listingPage(next string, releases ...string) string {
	body := `{"releases":[`
	for i, r := range releases {
		if i > 0 {
			body += ","
		}
		body += fmt.Sprintf(`{"id":%q,"ocid":"ocds-h6vhtk-000001","date":"2026-10-27T22:02:03Z","tag":["tender"]}`, r)
	}
	body += `],"links":{"next":`
	if next == "" {
		body += `""`
	} else {
		body += fmt.Sprintf("%q", next)
	}
	return body + `}}`
}

// TestSearchWalksTheDayOnceAndPagesFromIt: the service states no total, so the client walks the day, and
// the engine's numbered pages come from that one walk, with the same total on every page.
func TestSearchWalksTheDayOnceAndPagesFromIt(t *testing.T) {
	t.Parallel()
	var requests int
	var srvURL string
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		requests++
		if r.URL.Query().Get("cursor") == "" && r.URL.Query().Get("updatedFrom") == "" {
			t.Errorf("a listing request names no window: %s", r.URL)
		}
		if r.URL.Query().Get("cursor") == "" {
			_, _ = w.Write([]byte(listingPage(srvURL+"?cursor=two", "101800-2026", "101799-2026")))
			return
		}
		_, _ = w.Write([]byte(listingPage("", "101797-2026")))
	})
	srvURL = c.records.String()

	page := 1
	first, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(27), IssueDateTo: day(27), NumHitsPerPage: 2, Page: &page})
	if err != nil {
		t.Fatalf("Search page 1: %v", err)
	}
	if first.NumHitsTotal != 3 || len(first.Hits) != 2 || first.Hits[0].ID != "101800-2026" || first.Hits[0].PublicationDate != "2026-10-27" {
		t.Fatalf("page 1 = total %d, hits %+v", first.NumHitsTotal, first.Hits)
	}
	page = 2
	second, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(27), IssueDateTo: day(27), NumHitsPerPage: 2, Page: &page})
	if err != nil {
		t.Fatalf("Search page 2: %v", err)
	}
	if second.NumHitsTotal != 3 || len(second.Hits) != 1 || second.Hits[0].ID != "101797-2026" {
		t.Fatalf("page 2 = total %d, hits %+v", second.NumHitsTotal, second.Hits)
	}
	if requests != 2 {
		t.Errorf("the day was walked with %d requests, want 2 (one per listing page, once)", requests)
	}
	if !json.Valid(second.Raw) || second.ContentType != "application/json" {
		t.Errorf("the evidence is not the page's listing JSON: %q", second.Raw)
	}
}

// TestSearchRefusesALinkToAnotherHost: the service's next link is followed only on the service.
func TestSearchRefusesALinkToAnotherHost(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write([]byte(listingPage("https://elsewhere.example.com/listing", "101800-2026")))
	})
	page := 1
	_, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(27), IssueDateTo: day(27), NumHitsPerPage: 10, Page: &page})
	var decode *doffin.DecodeError
	if !errors.As(err, &decode) {
		t.Fatalf("err = %v, want a DecodeError for a link to another host", err)
	}
}

func TestSearchRefusesWhatTheServiceDoesNotAnswer(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Errorf("a refused search reached the service")
	})
	one := 1
	for name, p := range map[string]doffin.SearchParams{
		"two days": {IssueDateFrom: day(27), IssueDateTo: day(28), NumHitsPerPage: 10, Page: &one},
		"no day":   {NumHitsPerPage: 10, Page: &one},
		"page 0":   {IssueDateFrom: day(27), IssueDateTo: day(27), NumHitsPerPage: 10, Page: new(int)},
	} {
		if _, err := c.Search(context.Background(), p); !errors.Is(err, doffin.ErrInvalidParams) {
			t.Errorf("%s: err = %v, want ErrInvalidParams", name, err)
		}
	}
}

// TestDownloadKeepsTheReleasePackage: the notice is fetched by its number and kept as the service gave it.
func TestDownloadKeepsTheReleasePackage(t *testing.T) {
	t.Parallel()
	fixture, err := os.ReadFile("testdata/notice_101800.json")
	if err != nil {
		t.Fatal(err)
	}
	var path string
	c := newTestClient(t, func(w http.ResponseWriter, r *http.Request) {
		path = r.URL.Path
		_, _ = w.Write(fixture)
	})
	n, err := c.Download(context.Background(), "101800-2026")
	if err != nil {
		t.Fatalf("Download: %v", err)
	}
	if path != "/101800-2026" {
		t.Errorf("requested %q, want the notice number under the records endpoint", path)
	}
	sum := sha256.Sum256(fixture)
	if string(n.Body) != string(fixture) || n.SHA256 != hex.EncodeToString(sum[:]) || n.ContentType != "application/json" || n.ID != "101800-2026" {
		t.Errorf("notice %q (%s), hash %s; want the fixture kept whole", n.ID, n.ContentType, n.SHA256)
	}
}

// TestDownloadRefusesAPackageOfAnotherNotice: a package that does not hold the notice asked for is not
// stored as that notice.
func TestDownloadRefusesAPackageOfAnotherNotice(t *testing.T) {
	t.Parallel()
	fixture, err := os.ReadFile("testdata/notice_101800.json")
	if err != nil {
		t.Fatal(err)
	}
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		_, _ = w.Write(fixture)
	})
	_, err = c.Download(context.Background(), "101799-2026")
	var decode *doffin.DecodeError
	if !errors.As(err, &decode) {
		t.Fatalf("err = %v, want a DecodeError for another notice's package", err)
	}
}

func TestDownloadMapsNotFoundAndRefusesAForeignID(t *testing.T) {
	t.Parallel()
	missing := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusNotFound)
	})
	if _, err := missing.Download(context.Background(), "101800-2026"); !errors.Is(err, doffin.ErrNotFound) {
		t.Errorf("a 404: err = %v, want ErrNotFound", err)
	}
	refusing := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		t.Errorf("a download of a non-FTS id reached the service")
	})
	if _, err := refusing.Download(context.Background(), "2026-116564"); !errors.Is(err, doffin.ErrInvalidParams) {
		t.Errorf("a Doffin id: err = %v, want ErrInvalidParams", err)
	}
}

func TestSearchMapsTheServicesRateLimit(t *testing.T) {
	t.Parallel()
	c := newTestClient(t, func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Retry-After", "9")
		w.WriteHeader(http.StatusTooManyRequests)
	})
	page := 1
	_, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day(27), IssueDateTo: day(27), NumHitsPerPage: 10, Page: &page})
	var rl *doffin.RateLimitedError
	if !errors.As(err, &rl) || rl.RetryAfter != 9*time.Second {
		t.Errorf("429: err = %v, want a RateLimitedError that waits 9 seconds", err)
	}
}

func TestPublicationDatesReadTheListing(t *testing.T) {
	t.Parallel()
	body := []byte(listingPage("", "101800-2026"))
	dates := PublicationDates(body)
	if dates["101800-2026"] != "2026-10-27" || len(dates) != 1 {
		t.Errorf("dates = %v, want the release's day", dates)
	}
	if got := PublicationDates([]byte(`not json`)); len(got) != 0 {
		t.Errorf("a garbled listing gave dates %v", got)
	}
}

// TestDefaultDownloadPaceIsBelowTheServicesLimit: the service refused a burst of downloads faster than one
// every ten seconds, so the default is held there.
func TestDefaultDownloadPaceIsBelowTheServicesLimit(t *testing.T) {
	t.Parallel()
	c, err := New(Config{})
	if err != nil {
		t.Fatal(err)
	}
	if got := float64(c.download.Limit()); got != float64(DefaultDownloadRPM)/60 || DefaultDownloadRPM > 6 {
		t.Errorf("the default download pace is %v calls a second, want %d a minute (at most 6)", got, DefaultDownloadRPM)
	}
}

func TestNewRefusesAnInsecureURLAndAnOutOfRangePace(t *testing.T) {
	t.Parallel()
	if _, err := New(Config{RecordsURL: "http://fts.example.com/records"}); !errors.Is(err, ErrInvalidConfig) {
		t.Errorf("an http URL off loopback: err = %v, want ErrInvalidConfig", err)
	}
	if _, err := New(Config{SearchRPM: MaxRPM + 1}); !errors.Is(err, ErrInvalidConfig) {
		t.Errorf("a pace above the maximum: err = %v, want ErrInvalidConfig", err)
	}
}
