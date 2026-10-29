package http

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type registrySource struct{ registry *websites.Service }

func (s registrySource) WebsiteRegistry() *websites.Service { return s.registry }
func (s registrySource) Get(context.Context, string, string) pulse.Projection {
	return pulse.Projection{}
}

func TestWebsiteHTTPMappingsOAuthErrorsAndRateLimit(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "websites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	service := websites.NewService(repo, nil, nil)
	service.SetGoogleActions(websites.GoogleActions{
		Begin: func(context.Context) (websites.GoogleSession, error) {
			return websites.GoogleSession{SessionID: "session-one", AuthorizationURL: "https://accounts.google.com/o/oauth2/v2/auth", ExpiresAt: time.Now().Add(15 * time.Minute).UTC().Format(time.RFC3339)}, nil
		},
		Status: func(context.Context, string) (string, error) { return "connected", nil },
	})
	site, _, err := service.Add(context.Background(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	handler := NewHandler("key", registrySource{service})
	request := func(method, path, body string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, strings.NewReader(body))
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("X-Internal-API-Key", "key")
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	base := "/api/v1/marketing/websites/" + site.ID
	for _, tc := range []struct {
		method, path, body string
		status             int
	}{
		{"GET", "/api/v1/marketing/websites", "", 200},
		{"POST", "/api/v1/marketing/websites", `{"site":"example.com"}`, 200},
		{"GET", "/api/v1/marketing/websites/invalid", "", 404},
		{"PUT", base, "{}", 405},
		{"POST", base + "/unsupported", "{}", 404},
		{"POST", base + "/verify", "{}", 200},
		{"POST", base + "/google/connect", "{}", 200},
		{"GET", base + "/google/status?sessionId=session-one", "", 200},
		{"GET", base + "/google/status?sessionId=other", "", 404},
		{"GET", base + "/google/status?sessionId=session-one&extra=1", "", 400},
		{"POST", base + "/ownership/verify", "{}", 410},
		{"POST", base + "/ownership/challenge", "{}", 410},
		{"PATCH", base + "/mappings", `{"clarityProjectId":"abc123","revision":1}`, 409},
		{"PATCH", base, `{"site":"other.example","revision":1}`, 409},
	} {
		if w := request(tc.method, tc.path, tc.body); w.Code != tc.status {
			t.Fatalf("%s %s => %d %s", tc.method, tc.path, w.Code, w.Body.String())
		}
	}
	current, _ := service.Resolve(site.ID, "")
	payload, _ := json.Marshal(map[string]any{"clarityProjectId": "abc123", "revision": current.Website.Revision})
	if w := request("PATCH", base+"/mappings", string(payload)); w.Code != 200 {
		t.Fatalf("mapping: %d", w.Code)
	}
	current, _ = service.Resolve(site.ID, "")
	payload, _ = json.Marshal(map[string]any{"site": "https://other.example/", "revision": current.Website.Revision})
	if w := request("PATCH", base, string(payload)); w.Code != 200 {
		t.Fatalf("change: %d", w.Code)
	}
	if w := request("POST", base+"/verify", "not-json"); w.Code != 400 {
		t.Fatal("malformed JSON accepted")
	}
	for range 122 {
		request("GET", "/api/v1/marketing/websites", "")
	}
	if w := request("GET", "/api/v1/marketing/websites", ""); w.Code != 429 {
		t.Fatal("read requests not limited")
	}
}

func TestWebsiteHTTPAuthenticationStrictWritesAndBoundProjection(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "websites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	service := websites.NewService(repo, nil, nil)
	handler := NewHandler("test-key", registrySource{service})
	request := func(method, path, body, key string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, strings.NewReader(body))
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("X-Internal-API-Key", key)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	if w := request("GET", "/api/v1/marketing/websites", "", ""); w.Code != 401 {
		t.Fatalf("unauthorized: %d", w.Code)
	}
	for _, body := range []string{`{"site":"example.com","organizationId":"other"}`, `{"site":"example.com","site":"other.example"}`, `{"site":"https://localhost/"}`} {
		if w := request("POST", "/api/v1/marketing/websites", body, "test-key"); w.Code != 400 {
			t.Fatalf("invalid body accepted: %d %s", w.Code, w.Body.String())
		}
	}
	w := request("POST", "/api/v1/marketing/websites", `{"site":"example.com"}`, "test-key")
	if w.Code != 201 {
		t.Fatalf("add: %d %s", w.Code, w.Body.String())
	}
	var envelope struct {
		Data websites.Website `json:"data"`
	}
	_ = json.Unmarshal(w.Body.Bytes(), &envelope)
	id := envelope.Data.ID
	if w := request("GET", "/api/v1/marketing/pulse?site=https%3A%2F%2Fother.example%2F&websiteId="+id, "", "test-key"); w.Code != 409 {
		t.Fatalf("wrong site's identity accepted: %d", w.Code)
	}
	if w := request("GET", "/api/v1/marketing/websites/"+id, "", "test-key"); w.Code != 200 {
		t.Fatalf("read: %d", w.Code)
	}
	if w := request("PATCH", "/api/v1/marketing/websites/"+id, `{"site":"other.example","revision":0}`, "test-key"); w.Code != 400 {
		t.Fatalf("invalid revision accepted: %d", w.Code)
	}
	if w := request(http.MethodPost, "/api/v1/marketing/websites/"+id+"/verify", `{"property":"sc-domain:evil.example"}`, "test-key"); w.Code != 400 {
		t.Fatal("caller supplied provider mapping")
	}
	if w := request("POST", "/api/v1/marketing/websites/"+id+"/ownership/challenge", "{}", "test-key"); w.Code != 410 {
		t.Fatalf("challenge: %d", w.Code)
	}
}

func TestWebsiteHTTPGA4ReportingSiteMappingIsOwnerBoundAndStrict(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "websites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	service := websites.NewService(repo, func(context.Context, string) websites.Evidence {
		return websites.Evidence{SearchConsole: websites.Access{Status: "connected", ConnectionID: "gsc-one", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}, GA4: websites.Access{Status: "connected", ConnectionID: "ga4-one", PropertyID: "560102923", StreamID: "16177443323", MeasurementID: "G-ZEWPHF6X3K"}}
	}, nil)
	website, _, err := service.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	handler := NewHandler("private-key", registrySource{service})
	path := "/api/v1/marketing/websites/" + website.ID + "/mappings"
	request := func(body string) *httptest.ResponseRecorder {
		r := httptest.NewRequest("PATCH", path, strings.NewReader(body))
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("X-Internal-API-Key", "private-key")
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	for _, body := range []string{
		fmt.Sprintf(`{"ga4ReportingSite":"https://www.triodelab.no/","clarityProjectId":"abc123","revision":%d}`, website.Revision),
		fmt.Sprintf(`{"ga4ReportingSite":"https://www.triodelab.no/","propertyId":"560102923","revision":%d}`, website.Revision),
		fmt.Sprintf(`{"ga4ReportingSite":"https://www.triodelab.no/","ga4ReportingSite":"https://evil.example/","revision":%d}`, website.Revision),
		fmt.Sprintf(`{"ga4ReportingSite":"https://evil.example/","revision":%d}`, website.Revision),
	} {
		if w := request(body); w.Code != 400 {
			t.Fatalf("accepted unsafe mapping %d: %s", w.Code, body)
		}
	}
	payload, _ := json.Marshal(map[string]any{"ga4ReportingSite": "https://www.triodelab.no/", "revision": website.Revision})
	w := request(string(payload))
	if w.Code != 200 {
		t.Fatalf("mapping: %d %s", w.Code, w.Body.String())
	}
	var envelope struct {
		Data websites.Website `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &envelope); err != nil {
		t.Fatal(err)
	}
	if envelope.Data.Site != website.Site || envelope.Data.Integrations.GA4.ReportingSite != "https://www.triodelab.no/" || envelope.Data.Integrations.SearchConsole.Property != "sc-domain:triodelab.no" || strings.Contains(w.Body.String(), "ga4-one") {
		t.Fatalf("unsafe projection: %s", w.Body.String())
	}
	if w := request(string(payload)); w.Code != 409 {
		t.Fatalf("stale revision: %d", w.Code)
	}
}
