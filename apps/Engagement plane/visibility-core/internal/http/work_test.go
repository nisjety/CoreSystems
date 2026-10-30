package http

import (
	"context"
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

type queueSource struct {
	registrySource
	queue *workqueue.Service
}

func (s queueSource) WorkQueue() *workqueue.Service { return s.queue }
func (s queueSource) GetSearch(_ context.Context, quarter, site string) pulse.SearchProjection {
	period, _ := pulse.QuarterPeriod(quarter, time.Now())
	return pulse.SearchProjection{Site: site, Source: "google_search_console", Status: pulse.StatusConnected, Period: period, UpdatedAt: time.Now().UTC().Format(time.RFC3339), Opportunities: []pulse.Opportunity{{SearchRow: pulse.SearchRow{Key: "webdesign", Clicks: 2, Impressions: 200, CTR: 0.01, Position: 7}, Type: "title_meta"}}}
}
func TestWorkQueueOwnerHTTPValidatesAndPersists(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "websites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	site, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	queueRepo, err := workqueue.Open(filepath.Join(dir, "work.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	source := queueSource{registrySource: registrySource{registry: registry}}
	source.queue = workqueue.New(queueRepo, registry, source, nil)
	handler := NewHandler("test-key", source)
	request := func(method, path, body, key string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, strings.NewReader(body))
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("X-Internal-API-Key", key)
		w := httptest.NewRecorder()
		handler.ServeHTTP(w, r)
		return w
	}
	path := "/api/v1/marketing/work?websiteId=" + site.ID + "&site=https%3A%2F%2Ftriodelab.no%2F"
	if w := request("GET", path, "", ""); w.Code != 401 {
		t.Fatal("unauthorized queue exposed")
	}
	empty := request("GET", path, "", "test-key")
	if empty.Code != 200 || !strings.Contains(empty.Body.String(), `"tasks":[]`) || empty.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("empty=%d %s", empty.Code, empty.Body.String())
	}
	quarter := "Q" + string(rune('1'+int(time.Now().UTC().Month()-1)/3))
	websiteRevision := strconv.FormatInt(site.Revision, 10)
	body := `{"websiteId":"` + site.ID + `","site":"` + site.Site + `","websiteRevision":` + websiteRevision + `,"quarter":"` + quarter + `","query":"webdesign"}`
	w := request("POST", "/api/v1/marketing/work", body, "test-key")
	if w.Code != 201 {
		t.Fatalf("save=%d %s", w.Code, w.Body.String())
	}
	var value struct {
		Data workqueue.Snapshot `json:"data"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &value); err != nil {
		t.Fatal(err)
	}
	if len(value.Data.Tasks) != 1 || value.Data.Tasks[0].Evidence.Clicks != 2 {
		t.Fatalf("saved=%s", w.Body.String())
	}
	if w := request("POST", "/api/v1/marketing/work", body, "test-key"); w.Code != 200 {
		t.Fatalf("dedup=%d", w.Code)
	}
	task := value.Data.Tasks[0]
	change := `{"websiteId":"` + site.ID + `","site":"` + site.Site + `","websiteRevision":` + websiteRevision + `,"revision":1,"status":"done"}`
	if w := request("PATCH", "/api/v1/marketing/work/"+task.ID, change, "test-key"); w.Code != 200 || !strings.Contains(w.Body.String(), `"completionPercent":100`) {
		t.Fatalf("change=%d %s", w.Code, w.Body.String())
	}
	if w := request("PATCH", "/api/v1/marketing/work/"+task.ID, change, "test-key"); w.Code != 409 {
		t.Fatal("stale revision")
	}
	for _, tc := range []struct {
		method, path, body string
		want               int
	}{
		{"GET", path + "&websiteId=" + site.ID, "", 400}, {"GET", path + "&organizationId=other", "", 400},
		{"GET", "/api/v1/marketing/work?site=" + site.Site, "", 400},
		{"GET", strings.Replace(path, "triodelab.no", "other.no", 1), "", 409},
		{"POST", "/api/v1/marketing/work", strings.Replace(body, `"query":"webdesign"`, `"query":"missing"`, 1), 404},
		{"POST", "/api/v1/marketing/work", strings.Replace(body, `"websiteRevision":`+websiteRevision, `"websiteRevision":999`, 1), 409},
		{"POST", "/api/v1/marketing/work", strings.Replace(body, `"query":"webdesign"`, `"query":"webdesign","clicks":9999`, 1), 400},
		{"POST", "/api/v1/marketing/work", strings.Replace(body, `"query":"webdesign"`, `"query":"webdesign","query":"other"`, 1), 400},
		{"POST", "/api/v1/marketing/work?extra=1", body, 400},
		{"DELETE", "/api/v1/marketing/work/" + task.ID, "", 405},
		{"PATCH", "/api/v1/marketing/work/other", change, 404},
	} {
		if w := request(tc.method, tc.path, tc.body, "test-key"); w.Code != tc.want {
			t.Fatalf("%s %s=%d %s", tc.method, tc.path, w.Code, w.Body.String())
		}
	}
}
