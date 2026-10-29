package http

import (
	"context"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/webvitals"
)

type vitalsOwner struct {
	registrySource
	vitals *webvitals.Service
}

func (o vitalsOwner) WebVitals() *webvitals.Service { return o.vitals }

type vitalsCollector struct{}

func (vitalsCollector) Collect(_ context.Context, r websites.Record, environment string) (webvitals.Export, error) {
	w := r.Website
	return webvitals.Export{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Environment: environment, Status: "connected", ReportingOrigin: "https://triodelab.no", ObservedAt: time.Now().UTC().Format(time.RFC3339Nano), Samples: []webvitals.Sample{}}, nil
}
func TestWebVitalsStrictHTTP(t *testing.T) {
	dir := t.TempDir()
	_ = os.Chmod(dir, 0700)
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org_test")
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
	v, err := webvitals.New(filepath.Join(dir, "web-vitals.json"), "org_test", r, vitalsCollector{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	h := NewHandler("key", vitalsOwner{registrySource{r}, v})
	base := "/api/v1/marketing/web-vitals"
	query := "?websiteId=" + w.ID + "&site=https%3A%2F%2Ftriodelab.no%2F&environment=production"
	body := fmt.Sprintf(`{"websiteId":%q,"site":%q,"websiteRevision":%d,"environment":"production"}`, w.ID, w.Site, w.Revision)
	for _, tc := range []struct {
		method, path, body, key string
		status                  int
	}{
		{"GET", base + query, "", "", 401}, {"GET", base + query, "", "key", 200},
		{"GET", base + query + "&orgId=other", "", "key", 400}, {"GET", base + query + "&environment=production", "", "key", 400},
		{"GET", base + query, "{}", "key", 400}, {"GET", base, "", "key", 400},
		{"POST", base + "/sync?extra=1", body, "key", 400}, {"POST", base + "/sync", strings.TrimSuffix(body, "}") + `,"orgId":"bad"}`, "key", 400},
		{"POST", base + "/sync", strings.TrimSuffix(body, "}") + `,"environment":"production"}`, "key", 400},
		{"POST", base + "/sync", strings.Replace(body, `"environment":"production"`, `"environment":"staging"`, 1), "key", 400},
		{"POST", base + "/sync", body, "", 401}, {"POST", base + "/sync", body, "key", 200},
		{"POST", base + "/sync", body, "key", 429}, {"PUT", base + query, "", "key", 405}, {"GET", base + "/sync", "", "key", 405},
		{"POST", base + "/collect", body, "key", 404},
	} {
		req := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Internal-API-Key", tc.key)
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, req)
		if rec.Code != tc.status {
			t.Fatalf("%s %s got %d want %d: %s", tc.method, tc.path, rec.Code, tc.status, rec.Body.String())
		}
		if rec.Code == 200 && !strings.Contains(rec.Body.String(), `"source":"first_party_web_vitals"`) {
			t.Fatal(rec.Body.String())
		}
	}
}
