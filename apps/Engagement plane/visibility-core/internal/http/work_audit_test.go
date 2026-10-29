package http

import (
	"encoding/json"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

type workAuditStub struct {
	finding siteaudit.Finding
	check   siteaudit.CheckEvidence
}

func (s *workAuditStub) Finding(string, string, int64, string, string) (siteaudit.Finding, error) {
	return s.finding, nil
}
func (s *workAuditStub) LatestCheck(string, string, int64, string, string) (siteaudit.CheckEvidence, error) {
	return s.check, nil
}
func TestAuditWorkHTTPOnlyAcceptsServerFindingReferences(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(repo, nil, nil)
	w, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	queueRepo, err := workqueue.Open(filepath.Join(dir, "work.json"), "org-test")
	if err != nil {
		t.Fatal(err)
	}
	a := &workAuditStub{finding: siteaudit.Finding{ID: "finding_11111111111111111111111111111111", RuleID: "missing_title_signal", Severity: "warning", PageURL: w.Site, ObservedAt: "2026-10-22T10:00:00Z", RunID: "run_01K7WKF1W4A0KWXY74XRXBZ6TT", Fingerprint: "blake3:" + strings.Repeat("a", 64), RelatedURLs: []string{}}}
	a.check = siteaudit.CheckEvidence{AuditID: "audit_22222222222222222222222222222222", PageURL: w.Site, RuleID: a.finding.RuleID, Status: "passed", ObservedAt: "2026-10-22T11:00:00Z", RunID: a.finding.RunID, Fingerprint: a.finding.Fingerprint}
	source := queueSource{registrySource: registrySource{registry: registry}}
	source.queue = workqueue.New(queueRepo, registry, source, nil).WithAudit(a)
	handler := NewHandler("test-key", source)
	request := func(method, path, body, key string) *httptest.ResponseRecorder {
		r := httptest.NewRequest(method, path, strings.NewReader(body))
		r.Header.Set("Content-Type", "application/json")
		r.Header.Set("X-Internal-API-Key", key)
		out := httptest.NewRecorder()
		handler.ServeHTTP(out, r)
		return out
	}
	base := "/api/v1/marketing/work"
	websiteRevision := strconv.FormatInt(w.Revision, 10)
	body := `{"websiteId":"` + w.ID + `","site":"` + w.Site + `","websiteRevision":` + websiteRevision + `,"auditId":"audit_11111111111111111111111111111111","findingId":"` + a.finding.ID + `"}`
	if got := request("POST", base+"/audit", body, ""); got.Code != 401 {
		t.Fatalf("authentication %d", got.Code)
	}
	for _, bad := range []string{
		strings.TrimSuffix(body, "}") + `,"score":100}`,
		strings.TrimSuffix(body, "}") + `,"evidence":{"status":"passed"}}`,
		strings.Replace(body, `"websiteRevision":`+websiteRevision, `"websiteRevision":`+websiteRevision+`,"websiteRevision":999`, 1),
		strings.Replace(body, `"findingId":"`+a.finding.ID+`"`, `"findingId":"bad"`, 1),
	} {
		if got := request("POST", base+"/audit", bad, "test-key"); got.Code != 400 {
			t.Fatalf("forgery accepted %d %s", got.Code, got.Body.String())
		}
	}
	if got := request("GET", base+"/audit", "", "test-key"); got.Code != 405 {
		t.Fatalf("method %d", got.Code)
	}
	if got := request("POST", base+"/audit?extra=1", body, "test-key"); got.Code != 400 {
		t.Fatalf("query %d", got.Code)
	}
	got := request("POST", base+"/audit", body, "test-key")
	if got.Code != 201 {
		t.Fatalf("save %d %s", got.Code, got.Body.String())
	}
	var result struct {
		Data workqueue.Snapshot `json:"data"`
	}
	if err := json.Unmarshal(got.Body.Bytes(), &result); err != nil {
		t.Fatal(err)
	}
	verify := `{"websiteId":"` + w.ID + `","site":"` + w.Site + `","websiteRevision":` + websiteRevision + `,"revision":1}`
	got = request("POST", base+"/"+result.Data.Tasks[0].ID+"/verify", verify, "test-key")
	if got.Code != 200 || !strings.Contains(got.Body.String(), `"status":"passed"`) || !strings.Contains(got.Body.String(), `"status":"todo"`) {
		t.Fatalf("verify %d %s", got.Code, got.Body.String())
	}
}
