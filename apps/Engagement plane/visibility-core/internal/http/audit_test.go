package http

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type auditOwner struct {
	registrySource
	audit *siteaudit.Service
}

func (s auditOwner) Audit() *siteaudit.Service { return s.audit }

type blockedAudit struct{}

func (blockedAudit) Map(context.Context, string) ([]discovery.Page, error) {
	return nil, &discovery.CaptureError{Reason: "policy_blocked"}
}
func (blockedAudit) Capture(context.Context, string, string) (siteaudit.CapturePage, error) {
	panic("unexpected capture")
}
func TestAuditHTTPAuthorizesAndRejectsForgedContexts(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	site, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	audits, err := siteaudit.Open(filepath.Join(dir, "audits.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	owner := auditOwner{registrySource: registrySource{registry: registry}, audit: siteaudit.New(audits, registry, nil, nil)}
	handler := NewHandler("test-key", owner)
	base := "/api/v1/marketing/audits"
	query := "?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	body := `{"websiteId":"` + site.ID + `","site":"` + site.Site + `","websiteRevision":` + strconv.FormatInt(site.Revision, 10) + `}`
	for _, tc := range []struct {
		method, path, body, key, media string
		status                         int
	}{
		{"GET", base + query, "", "", "", 401}, {"GET", base + query, "", "test-key", "", 200}, {"GET", base + query + "&auditId=invalid", "", "test-key", "", 400}, {"GET", base + query + "&auditId=audit_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "", "test-key", "", 404}, {"GET", base + query + "&organizationId=other", "", "test-key", "", 400}, {"GET", base + query + "&site=https%3A%2F%2Fevil.test%2F", "", "test-key", "", 400}, {"GET", base + "?websiteId=" + site.ID + "&site=https%3A%2F%2Fother.no%2F", "", "test-key", "", 409}, {"POST", base, body, "test-key", "application/json", 200}, {"POST", base, body, "test-key", "text/plain", 415}, {"POST", base + query, body, "test-key", "application/json", 400}, {"POST", base, strings.TrimSuffix(body, "}") + `,"findings":[]}`, "test-key", "application/json", 400}, {"POST", base, strings.Replace(body, `"websiteRevision":2`, `"websiteRevision":999`, 1), "test-key", "application/json", 409}, {"POST", base, strings.TrimSuffix(body, "}") + `,"websiteRevision":2}`, "test-key", "application/json", 400}, {"DELETE", base, "", "test-key", "", 405},
	} {
		r := httptest.NewRequest(tc.method, tc.path, strings.NewReader(tc.body))
		r.Header.Set("X-Internal-API-Key", tc.key)
		r.Header.Set("Content-Type", tc.media)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		if w.Code != tc.status {
			t.Fatalf("%s %s got%d want%d %s", tc.method, tc.path, w.Code, tc.status, w.Body.String())
		}
		if w.Code == 200 {
			if w.Header().Get("Cache-Control") != "no-store" || strings.Contains(w.Body.String(), "organizationId") {
				t.Fatal("private cache/org exposed")
			}
			var result struct {
				Data siteaudit.Snapshot `json:"data"`
			}
			if json.Unmarshal(w.Body.Bytes(), &result) != nil || result.Data.Source != "quarry_capture" || result.Data.Reason != "not_configured" || len(result.Data.Pages) != 0 {
				t.Fatal("fabricated audit", w.Body.String())
			}
		}
	}
	owner.audit = siteaudit.New(audits, registry, blockedAudit{}, nil)
	r := httptest.NewRequest(http.MethodPost, base, strings.NewReader(body))
	r.Header.Set("X-Internal-API-Key", "test-key")
	r.Header.Set("Content-Type", "application/json")
	w := httptest.NewRecorder()
	NewHandler("test-key", owner).ServeHTTP(w, r)
	if w.Code != 200 || !strings.Contains(w.Body.String(), `"status":"running"`) {
		t.Fatal("async start", w.Code, w.Body.String())
	}
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		value, err := owner.audit.Get(site.ID, site.Site)
		if err != nil {
			t.Fatal(err)
		}
		if value.Status != "running" {
			return
		}
		runtime.Gosched()
	}
	t.Fatal("async audit did not finish")
}
