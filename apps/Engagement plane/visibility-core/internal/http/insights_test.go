package http

import (
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

type insightOwner struct {
	auditOwner
	work *workqueue.Service
}

func (s insightOwner) WorkQueue() *workqueue.Service { return s.work }

func TestInsightHTTPReadOnlyStrictContextsAndCurrentTasks(t *testing.T) {
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
	clock := func() time.Time { return time.Date(2026, 10, 22, 12, 0, 0, 0, time.UTC) }
	owner := insightOwner{auditOwner: auditOwner{registrySource: registrySource{registry: registry}, audit: siteaudit.New(audits, registry, nil, clock)}}
	query := "?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	get := func(method, path, key string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, nil)
		r.Header.Set("X-Internal-API-Key", key)
		w := httptest.NewRecorder()
		NewHandler("test-key", owner).ServeHTTP(w, r)
		return w
	}
	for _, path := range []string{"/api/v1/marketing/audit-insights", "/api/v1/marketing/reports/weekly"} {
		for _, tc := range []struct {
			method, suffix, key string
			status              int
		}{{"GET", query, "", 401}, {"GET", query, "test-key", 200}, {"POST", query, "test-key", 405}, {"GET", query + "&org=foreign", "test-key", 400}, {"GET", query + "&site=https%3A%2F%2Fevil.no%2F", "test-key", 400}, {"GET", strings.Replace(query, "triodelab.no", "other.no", 1), "test-key", 409}} {
			w := get(tc.method, path+tc.suffix, tc.key)
			if w.Code != tc.status {
				t.Fatalf("%s %s %d %s", tc.method, path+tc.suffix, w.Code, w.Body.String())
			}
			if w.Code == 200 && (w.Header().Get("Cache-Control") != "no-store" || strings.Contains(w.Body.String(), "organizationId")) {
				t.Fatal("private data/cache")
			}
		}
	}
	for _, suffix := range []string{"&weekStart=2026-10-20", "&weekStart=2026-10-26", "&weekStart=2026-02-30", "&weekStart=", "&weekStart=2026-10-19&weekStart=2026-10-19", "&weekStart=%ZZ"} {
		if w := get("GET", "/api/v1/marketing/reports/weekly"+query+suffix, "test-key"); w.Code != 400 {
			t.Fatal("invalid week accepted", suffix, w.Code)
		}
	}
	empty := get("GET", "/api/v1/marketing/reports/weekly"+query, "test-key")
	if !strings.Contains(empty.Body.String(), `"work":null`) || !strings.Contains(empty.Body.String(), `"observed":false`) {
		t.Fatal("fabricated empty metrics", empty.Body.String())
	}
	queue, err := workqueue.Open(filepath.Join(dir, "work.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	owner.work = workqueue.New(queue, registry, nil, clock)
	response := get("GET", "/api/v1/marketing/reports/weekly"+query+"&weekStart=2026-10-12", "test-key")
	var decoded struct {
		Data struct {
			TasksAsOf string             `json:"tasksAsOf"`
			Work      *workqueue.Summary `json:"work"`
			Status    string             `json:"status"`
		}
	}
	if response.Code != 200 || json.Unmarshal(response.Body.Bytes(), &decoded) != nil || decoded.Data.Work == nil || decoded.Data.Work.Total != 0 || decoded.Data.TasksAsOf != "2026-10-22T12:00:00Z" || decoded.Data.Status != "closed" {
		t.Fatal("current task snapshot missing", response.Body.String())
	}
}
