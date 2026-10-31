package ted

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/anbud/internal/sources/doffin"
)

var day = time.Date(2026, 10, 27, 0, 0, 0, 0, time.UTC)

// fake serves TED's two endpoints on loopback. search answers the search API with the request it was
// given; notice answers a notice's XML endpoint.
func fake(t *testing.T, search func(req searchRequest) (int, string), notice func(id string) (int, http.Header, string)) *Client {
	t.Helper()
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch {
		case r.Method == http.MethodPost && r.URL.Path == "/v3/notices/search":
			var req searchRequest
			body, _ := io.ReadAll(r.Body)
			if err := json.Unmarshal(body, &req); err != nil {
				t.Errorf("search body is not JSON: %v", err)
			}
			code, out := search(req)
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(code)
			_, _ = io.WriteString(w, out)
		case r.Method == http.MethodGet && strings.HasPrefix(r.URL.Path, "/en/notice/") && strings.HasSuffix(r.URL.Path, "/xml"):
			code, header, out := notice(strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, "/en/notice/"), "/xml"))
			for k, v := range header {
				w.Header()[k] = v
			}
			w.WriteHeader(code)
			_, _ = io.WriteString(w, out)
		default:
			t.Errorf("unexpected request %s %s", r.Method, r.URL.Path)
			w.WriteHeader(http.StatusTeapot)
		}
	}))
	t.Cleanup(srv.Close)
	c, err := New(Config{SearchURL: srv.URL + "/v3/notices/search", NoticeURL: srv.URL, Countries: []string{"SWE", "DNK"}, SearchRPM: MaxRPM, DownloadRPM: MaxRPM})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return c
}

func page(n int) *int { return &n }

func TestASearchIsOneDayForTheConfiguredBuyers(t *testing.T) {
	var got searchRequest
	c := fake(t, func(req searchRequest) (int, string) {
		got = req
		return 200, `{"notices":[{"publication-number":"741144-2026"},{"publication-number":"741190-2026"}],"totalNoticeCount":231}`
	}, nil)
	resp, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day, IssueDateTo: day, NumHitsPerPage: 100, Page: page(2)})
	if err != nil {
		t.Fatalf("Search: %v", err)
	}
	if got.Query != "publication-date=20261027 AND buyer-country IN (SWE DNK)" || got.Limit != 100 || got.Page != 2 || got.Scope != "ALL" {
		t.Errorf("request %+v", got)
	}
	if resp.NumHitsTotal != 231 || resp.NumHitsAccessible != 231 || len(resp.Hits) != 2 || resp.Hits[0].ID != "741144-2026" || len(resp.Raw) == 0 || resp.FetchedAt.IsZero() {
		t.Errorf("response %+v", resp)
	}
}

func TestASearchOfMoreThanOneDayOrAnOddPageIsRefusedBeforeAnyCall(t *testing.T) {
	c := fake(t, func(searchRequest) (int, string) {
		t.Error("TED was called")
		return 500, ""
	}, nil)
	for _, p := range []doffin.SearchParams{
		{IssueDateFrom: day, IssueDateTo: day.AddDate(0, 0, 1), NumHitsPerPage: 100},
		{NumHitsPerPage: 100},
		{IssueDateFrom: day, IssueDateTo: day, NumHitsPerPage: 101},
		{IssueDateFrom: day, IssueDateTo: day, NumHitsPerPage: 10, Page: page(0)},
	} {
		if _, err := c.Search(context.Background(), p); !errors.Is(err, doffin.ErrInvalidParams) {
			t.Errorf("%+v: err %v, want ErrInvalidParams", p, err)
		}
	}
}

func TestASearchResponseWithoutACountIsADecodeError(t *testing.T) {
	c := fake(t, func(searchRequest) (int, string) { return 200, `{"notices":[]}` }, nil)
	var de *doffin.DecodeError
	if _, err := c.Search(context.Background(), doffin.SearchParams{IssueDateFrom: day, IssueDateTo: day, NumHitsPerPage: 10}); !errors.As(err, &de) {
		t.Errorf("err %v, want a DecodeError", err)
	}
}

func TestDownloadOutcomes(t *testing.T) {
	const xmlBody = `<?xml version="1.0" encoding="UTF-8"?><ContractNotice/>`
	for _, tc := range []struct {
		name   string
		code   int
		header http.Header
		body   string
		check  func(t *testing.T, n doffin.Notice, err error)
	}{
		{"the notice", 200, http.Header{"Content-Type": {"application/xml"}}, xmlBody, func(t *testing.T, n doffin.Notice, err error) {
			if err != nil || n.ID != "741144-2026" || string(n.Body) != xmlBody || len(n.SHA256) != 64 || n.FetchedAt.IsZero() {
				t.Errorf("%+v, %v", n, err)
			}
		}},
		{"not found", 404, nil, "", func(t *testing.T, _ doffin.Notice, err error) {
			if !errors.Is(err, doffin.ErrNotFound) {
				t.Errorf("err %v, want ErrNotFound", err)
			}
		}},
		{"rate limited", 429, http.Header{"Retry-After": {"7"}}, "<html>429</html>", func(t *testing.T, _ doffin.Notice, err error) {
			var rl *doffin.RateLimitedError
			if !errors.As(err, &rl) || rl.RetryAfter != 7*time.Second {
				t.Errorf("err %v, want a RateLimitedError of 7s", err)
			}
		}},
		{"an HTML page with a success status", 200, nil, "<html><body>maintenance</body></html>", func(t *testing.T, _ doffin.Notice, err error) {
			var he *doffin.HTTPError
			if !errors.As(err, &he) {
				t.Errorf("err %v, want an HTTPError, not a stored page", err)
			}
		}},
		{"a server error", 503, nil, "down\x1b[31m for now", func(t *testing.T, _ doffin.Notice, err error) {
			var he *doffin.HTTPError
			if !errors.As(err, &he) || he.Status != 503 || strings.ContainsRune(he.Message, '\x1b') {
				t.Errorf("err %v, want a sanitised HTTPError 503", err)
			}
		}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			c := fake(t, nil, func(id string) (int, http.Header, string) {
				if id != "741144-2026" {
					t.Errorf("asked for %q", id)
				}
				return tc.code, tc.header, tc.body
			})
			n, err := c.Download(context.Background(), "741144-2026")
			tc.check(t, n, err)
		})
	}
}

// A connection that drops is a transport failure, not an HTTP status nobody sent.
func TestADroppedConnectionIsNotAnHTTPError(t *testing.T) {
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		conn, _, err := w.(http.Hijacker).Hijack()
		if err == nil {
			_ = conn.Close()
		}
	}))
	t.Cleanup(srv.Close)
	c, err := New(Config{SearchURL: srv.URL, NoticeURL: srv.URL, SearchRPM: MaxRPM, DownloadRPM: MaxRPM})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	_, err = c.Download(context.Background(), "741144-2026")
	var he *doffin.HTTPError
	if err == nil || errors.As(err, &he) || errors.Is(err, doffin.ErrInvalidParams) {
		t.Errorf("err %v, want a plain transport error", err)
	}
}

func TestAMalformedPublicationNumberIsRefusedBeforeAnyCall(t *testing.T) {
	c := fake(t, nil, func(string) (int, http.Header, string) {
		t.Error("TED was called")
		return 500, nil, ""
	})
	for _, id := range []string{"2026-741144x", "../x", "741144-26", ""} {
		if _, err := c.Download(context.Background(), id); !errors.Is(err, doffin.ErrInvalidParams) {
			t.Errorf("%q: err %v, want ErrInvalidParams", id, err)
		}
	}
}

func TestTheConfigurationIsChecked(t *testing.T) {
	for _, cfg := range []Config{
		{Countries: []string{"swe"}},
		{Countries: []string{"SWE) OR (1=1"}},
		{SearchRPM: MaxRPM + 1},
		{SearchURL: "http://api.ted.europa.eu/v3/notices/search"},
		{NoticeURL: "ted.europa.eu"},
	} {
		if _, err := New(cfg); !errors.Is(err, ErrInvalidConfig) {
			t.Errorf("%+v: err %v, want ErrInvalidConfig", cfg, err)
		}
	}
	c, err := New(Config{})
	if err != nil || c.Query(day) != "publication-date=20261027 AND buyer-country IN (NOR SWE DNK FIN ISL)" {
		t.Errorf("defaults: %v, query %q", err, c.Query(day))
	}
}
