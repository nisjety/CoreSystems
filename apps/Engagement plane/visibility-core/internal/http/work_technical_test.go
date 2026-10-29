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

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

type technicalQueueSource struct {
	queueSource
	report *pulse.PageSpeedProjection
}

func (s technicalQueueSource) GetPageSpeed(context.Context, string, string) pulse.PageSpeedProjection {
	return *s.report
}
func TestTechnicalWorkOwnerHTTPAuthorizesValidatesAndVerifies(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	website, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	queueRepo, err := workqueue.Open(filepath.Join(dir, "work.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	projection := pulse.PageSpeedProjection{Site: website.Site, Source: "google_pagespeed", Status: "complete", Strategy: "desktop", Report: &pulse.PageSpeedReport{FetchTime: "2026-10-22T10:00:00Z", LighthouseVersion: "13.0.0", Checks: []pulse.PageSpeedCheck{{ID: "structured-data", Status: "manual"}}}}
	source := technicalQueueSource{queueSource: queueSource{registrySource: registrySource{registry: registry}}, report: &projection}
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
	base := "/api/v1/marketing/work"
	websiteRevision := strconv.FormatInt(website.Revision, 10)
	body := `{"websiteId":"` + website.ID + `","site":"` + website.Site + `","websiteRevision":` + websiteRevision + `,"strategy":"desktop","checkId":"structured-data","fetchTime":"2026-10-22T10:00:00Z"}`
	if got := request("POST", base+"/technical", body, ""); got.Code != 401 {
		t.Fatalf("unauthorized %d", got.Code)
	}
	for _, tc := range []struct {
		path, body string
		want       int
	}{
		{base + "/technical", strings.Replace(body, `"strategy":"desktop"`, `"strategy":"desktop","checkScore":1`, 1), 400},
		{base + "/technical", strings.Replace(body, `"checkId":"structured-data"`, `"checkId":"structured-data","checkId":"canonical"`, 1), 400},
		{base + "/technical?extra=1", body, 400},
		{base + "/technical", strings.Replace(body, `"strategy":"desktop"`, `"strategy":"tablet"`, 1), 400},
		{base + "/technical", strings.Replace(body, `"websiteRevision":`+websiteRevision, `"websiteRevision":999`, 1), 409},
		{base + "/technical", strings.Replace(body, "2026-10-22T10:00:00Z", "2026-10-21T10:00:00Z", 1), 409},
	} {
		got := request("POST", tc.path, tc.body, "test-key")
		if got.Code != tc.want {
			t.Fatalf("%s=%d want%d %s", tc.path, got.Code, tc.want, got.Body.String())
		}
	}
	got := request("POST", base+"/technical", body, "test-key")
	if got.Code != 201 {
		t.Fatalf("create=%d %s", got.Code, got.Body.String())
	}
	var value struct {
		Data workqueue.Snapshot `json:"data"`
	}
	if err := json.Unmarshal(got.Body.Bytes(), &value); err != nil {
		t.Fatal(err)
	}
	task := value.Data.Tasks[0]
	verify := `{"websiteId":"` + website.ID + `","site":"` + website.Site + `","websiteRevision":` + websiteRevision + `,"revision":1}`
	path := base + "/" + task.ID + "/verify"
	if got := request("POST", path, verify, "test-key"); got.Code != 409 {
		t.Fatalf("old verification %d", got.Code)
	}
	if got := request("POST", path, strings.TrimSuffix(verify, "}")+`,"score":1}`, "test-key"); got.Code != 400 {
		t.Fatalf("forged verification %d", got.Code)
	}
	if got := request("GET", path, "", "test-key"); got.Code != 405 {
		t.Fatalf("verify method %d", got.Code)
	}
	pass := 1.0
	projection.Report.FetchTime = "2026-10-22T11:00:00Z"
	projection.Report.Checks[0] = pulse.PageSpeedCheck{ID: "structured-data", Status: "passed", Score: &pass}
	got = request("POST", path, verify, "test-key")
	if got.Code != 200 || !strings.Contains(got.Body.String(), `"kind":"verified"`) || !strings.Contains(got.Body.String(), `"status":"todo"`) {
		t.Fatalf("verify=%d %s", got.Code, got.Body.String())
	}
	if got := request("POST", path, verify, "test-key"); got.Code != 409 {
		t.Fatalf("stale verification revision %d", got.Code)
	}
}
