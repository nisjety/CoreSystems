package http

import (
	"bytes"
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/trends"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type trendsHTTPFixture struct {
	calls int
	geo   string
}

type automaticHTTPSource struct {
	registrySource
	trendsHTTPFixture
	automatic *trends.Automatic
}

func (s automaticHTTPSource) AutomaticTrends() *trends.Automatic { return s.automatic }
func (s automaticHTTPSource) TrendsSearch(context.Context, websites.Record) (pulse.SearchProjection, error) {
	return pulse.SearchProjection{}, nil
}
func TestAutomaticTrendsHTTPReadIsAuthenticatedScopedAndDoesNotCreateWork(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "websites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	w, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	source := automaticHTTPSource{registrySource: registrySource{registry: registry}}
	path := filepath.Join(dir, "trends.json")
	source.automatic, err = trends.NewAutomatic(path, "org-test", registry, &source, nil)
	if err != nil {
		t.Fatal(err)
	}
	before, _ := os.ReadFile(path)
	handler := NewHandler("server-key", &source)
	base := "/api/v1/marketing/trends/automatic?websiteId=" + w.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	for _, tc := range []struct {
		method, path, key string
		status            int
	}{
		{"GET", base, "", 401}, {"GET", base, "server-key", 200}, {"GET", base + "&orgId=forged", "server-key", 400}, {"GET", strings.Replace(base, "triodelab.no", "other.no", 1), "server-key", 409}, {"POST", base, "server-key", 405},
	} {
		req := httptest.NewRequest(tc.method, tc.path, nil)
		req.Header.Set("X-Internal-API-Key", tc.key)
		out := httptest.NewRecorder()
		handler.ServeHTTP(out, req)
		if out.Code != tc.status {
			t.Fatalf("status %d want %d", out.Code, tc.status)
		}
	}
	after, _ := os.ReadFile(path)
	if !bytes.Equal(before, after) {
		t.Fatal("dashboard read created work or changed storage")
	}
}

func (f *trendsHTTPFixture) GetTrends(_ context.Context, _ string, _ string, _ int64, q trends.Query) (trends.Report, error) {
	f.calls++
	f.geo = q.Geo
	return trends.Report{Query: q, Status: "no_data", Rows: []trends.Row{}}, nil
}
func TestTrendsHTTPClosedInputAndWorldwideMarket(t *testing.T) {
	good := `{"websiteId":"web_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","site":"https://triodelab.no/","websiteRevision":2,"terms":["SEO"],"geo":"","timeframe":"today 12-m"}`
	for _, tc := range []struct {
		path, body string
		want       int
	}{
		{"/api/v1/marketing/trends", good, 200},
		{"/api/v1/marketing/trends?orgId=other", good, 400},
		{"/api/v1/marketing/trends", strings.Replace(good, `"terms":["SEO"]`, `"terms":["SEO","SEO"]`, 1), 400},
		{"/api/v1/marketing/trends", strings.Replace(good, `"geo":""`, `"geo":"","geo":"NO"`, 1), 400},
	} {
		fixture := &trendsHTTPFixture{}
		r := httptest.NewRequest("POST", tc.path, strings.NewReader(tc.body))
		r.Header.Set("Content-Type", "application/json")
		w := httptest.NewRecorder()
		trendsHandler(fixture)(w, r)
		if w.Code != tc.want || tc.want != 200 && fixture.calls != 0 || tc.want == 200 && fixture.geo != "" {
			t.Fatalf("status %d want %d calls %d", w.Code, tc.want, fixture.calls)
		}
	}
}
