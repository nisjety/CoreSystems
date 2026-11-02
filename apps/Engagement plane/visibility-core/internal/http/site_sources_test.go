package http

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type siteSourcesOwner struct {
	registrySource
	sources *sitesources.Service
}
type sourceDeadlineWriter struct {
	*httptest.ResponseRecorder
	deadlines     []time.Time
	deadlineError error
}

func (w *sourceDeadlineWriter) SetWriteDeadline(at time.Time) error {
	w.deadlines = append(w.deadlines, at)
	return w.deadlineError
}
func TestSiteSourcesWriteDeadlineIsScopedAndFailuresAreSanitized(t *testing.T) {
	for _, tc := range []struct {
		method        string
		deadlineErr   error
		status, calls int
	}{
		{"POST", nil, 400, 1}, {"POST", http.ErrNotSupported, 400, 1}, {"POST", errors.New("private timeout detail"), 503, 1}, {"GET", nil, 405, 0},
	} {
		req := httptest.NewRequest(tc.method, "/api/v1/marketing/site-sources/sync", strings.NewReader("{}"))
		req.Header.Set("Content-Type", "application/json")
		w := &sourceDeadlineWriter{ResponseRecorder: httptest.NewRecorder(), deadlineError: tc.deadlineErr}
		start := time.Now()
		siteSourcesSync(nil)(w, req)
		if w.Code != tc.status || len(w.deadlines) != tc.calls || strings.Contains(w.Body.String(), "private timeout") {
			t.Fatal(tc, w.Code, w.Body.String())
		}
		if tc.calls > 0 && (w.deadlines[0].Before(start.Add(37*time.Second)) || w.deadlines[0].After(start.Add(39*time.Second))) {
			t.Fatal(w.deadlines)
		}
	}
}

func (s siteSourcesOwner) SiteSources() *sitesources.Service { return s.sources }

type siteSourcesCollector struct{}

func (siteSourcesCollector) Collect(_ context.Context, r websites.Record, p string) (sitesources.Report, error) {
	u := time.Now().UTC()
	day := time.Date(u.Year(), u.Month(), u.Day(), 0, 0, 0, 0, time.UTC)
	w := r.Website
	return sitesources.Report{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Provider: p, Status: "not_configured", ObservedAt: u.Format(time.RFC3339Nano), Window: sitesources.Window{StartDate: day.AddDate(0, 0, -7).Format("2006-01-02"), EndDate: day.AddDate(0, 0, -1).Format("2006-01-02")}}, nil
}
func TestSiteSourcesStrictHTTP(t *testing.T) {
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org_sites")
	if err != nil {
		t.Fatal(err)
	}
	r := websites.NewService(repo, func(context.Context, string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "conn-a", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}}
	}, nil)
	w, _, err := r.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	s, err := sitesources.New(filepath.Join(dir, "site-sources.json"), "org_sites", r, siteSourcesCollector{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	h := NewHandler("key", siteSourcesOwner{registrySource{r}, s})
	base := "/api/v1/marketing/site-sources"
	query := "?websiteId=" + w.ID + "&site=https%3A%2F%2Ftriodelab.no%2F&provider=algolia"
	body := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d,"provider":"algolia"}`, w.ID, w.Site, w.Revision)
	for _, tc := range []struct {
		method, path, body, key string
		status                  int
	}{
		{"GET", base + query, "", "", 401}, {"GET", base + query, "", "key", 200},
		{"GET", base + query + "&orgId=other", "", "key", 400}, {"GET", base + query + "&provider=cookiebot", "", "key", 400},
		{"GET", base + query, "{}", "key", 400}, {"GET", base, "", "key", 400},
		{"POST", base + "/sync?extra=1", body, "key", 400}, {"POST", base + "/sync", strings.TrimSuffix(body, "}") + `,"apiKey":"bad"}`, "key", 400},
		{"POST", base + "/sync", strings.TrimSuffix(body, "}") + `,"provider":"algolia"}`, "key", 400},
		{"POST", base + "/sync", strings.Replace(body, `"provider":"algolia"`, `"provider":"other"`, 1), "key", 400},
		{"POST", base + "/sync", body, "", 401}, {"POST", base + "/sync", body, "key", 200},
		{"POST", base + "/sync", body, "key", 429}, {"PUT", base + query, "", "key", 405}, {"GET", base + "/sync", "", "key", 405},
	} {
		req := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", tc.key)
		res := httptest.NewRecorder()
		h.ServeHTTP(res, req)
		if res.Code != tc.status {
			t.Fatalf("%s %s => %d want %d %s", tc.method, tc.path, res.Code, tc.status, res.Body.String())
		}
		if res.Code == 200 && (!strings.Contains(res.Body.String(), `"provider":"algolia"`) || strings.Contains(res.Body.String(), "analytics")) {
			t.Fatal("fabricated metrics", res.Body.String())
		}
	}
}
