package workqueue

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

const auditOne = "audit_11111111111111111111111111111111"
const auditTwo = "audit_22222222222222222222222222222222"
const findingOne = "finding_11111111111111111111111111111111"

type auditStub struct {
	finding siteaudit.Finding
	check   siteaudit.CheckEvidence
	err     error
	during  func()
	calls   int
}

func (a *auditStub) Finding(string, string, int64, string, string) (siteaudit.Finding, error) {
	a.calls++
	if a.during != nil {
		a.during()
	}
	return a.finding, a.err
}
func (a *auditStub) LatestCheck(string, string, int64, string, string) (siteaudit.CheckEvidence, error) {
	a.calls++
	if a.during != nil {
		a.during()
	}
	return a.check, a.err
}
func auditService(t *testing.T) (*Service, *websites.Service, websites.Website, *auditStub, string) {
	t.Helper()
	s, registry, w, _, path := testService(t)
	a := &auditStub{finding: siteaudit.Finding{ID: findingOne, RuleID: "missing_title_signal", Severity: "warning", PageURL: w.Site + "innsikt", ObservedAt: "2026-10-22T10:00:00Z", RunID: "run_01K7WKF1W4A0KWXY74XRXBZ6TT", Fingerprint: "blake3:" + strings.Repeat("a", 64), RelatedURLs: []string{}}}
	a.check = siteaudit.CheckEvidence{AuditID: auditTwo, PageURL: a.finding.PageURL, RuleID: a.finding.RuleID, Status: "passed", ObservedAt: "2026-10-22T11:00:00Z", RunID: a.finding.RunID, Fingerprint: a.finding.Fingerprint}
	return s.WithAudit(a), registry, w, a, path
}
func saveAudit(t *testing.T, s *Service, w websites.Website) Snapshot {
	t.Helper()
	value, created, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditOne, findingOne)
	if err != nil || !created {
		t.Fatalf("audit save created=%v err=%v", created, err)
	}
	return value
}
func TestAuditWorkPersistsDeduplicatesAndKeepsVerificationHistory(t *testing.T) {
	s, registry, w, a, path := auditService(t)
	value := saveAudit(t, s, w)
	task := value.Tasks[0]
	if task.Evidence.Kind != "site_audit_finding" || task.Evidence.Audit == nil || task.Evidence.Audit.AuditID != auditOne {
		t.Fatalf("evidence %+v", task.Evidence)
	}
	if _, created, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditOne, findingOne); err != nil || created {
		t.Fatalf("dedupe %v %v", created, err)
	}
	value, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1)
	if err != nil || value.Tasks[0].Status != "todo" || value.Tasks[0].Verification.Status != "passed" || value.Tasks[0].Verification.Audit.AuditID != auditTwo || value.Summary.Done != 0 {
		t.Fatalf("pass %+v %v", value, err)
	}
	a.check.ObservedAt = "2026-10-22T11:30:00Z"
	a.check.AuditID = "audit_33333333333333333333333333333333"
	a.check.Status = "inconclusive"
	value, err = s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 2)
	if err != nil || len(value.Activity) != 3 || value.Activity[1].Verification.Status != "passed" || value.Tasks[0].Verification.Status != "inconclusive" {
		t.Fatalf("history %+v %v", value, err)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, field := range []string{"score", "lighthouseVersion", "clicks", "period", "checkScore"} {
		if strings.Contains(string(raw), `"`+field+`"`) {
			t.Fatalf("invented field %s: %s", field, raw)
		}
	}
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	value, err = New(repo, registry, nil, nil).List(w.ID, w.Site)
	if err != nil || value.Tasks[0].Revision != 3 || value.Tasks[0].Verification.Status != "inconclusive" {
		t.Fatalf("restart %+v %v", value, err)
	}
}
func TestAuditRejectsInvalidStaleForeignAndFutureEvidence(t *testing.T) {
	for _, change := range []func(*auditStub){
		func(a *auditStub) { a.finding.PageURL = "https://other.no/page" },
		func(a *auditStub) { a.finding.PageURL = "https://triodelab.no/login" },
		func(a *auditStub) { a.finding.PageURL += "?token=private" },
		func(a *auditStub) { a.finding.RuleID = "unknown-rule" },
		func(a *auditStub) { a.finding.Severity = "critical" },
		func(a *auditStub) { a.finding.ID = "finding_22222222222222222222222222222222" },
		func(a *auditStub) { a.finding.ObservedAt = "2026-10-22T13:00:00Z" },
		func(a *auditStub) { a.finding.Fingerprint = "fake" },
		func(a *auditStub) { a.finding.RunID = "fake" },
		func(a *auditStub) { a.finding.RelatedURLs = []string{"https://other.no/"} },
	} {
		s, _, w, a, _ := auditService(t)
		change(a)
		if _, _, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditOne, findingOne); err == nil {
			t.Fatal("untrusted evidence saved")
		}
		value, _ := s.List(w.ID, w.Site)
		if len(value.Tasks) != 0 {
			t.Fatal("invalid evidence mutated queue")
		}
	}
	s, _, w, a, _ := auditService(t)
	if _, _, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision+1, auditOne, findingOne); !errors.Is(err, ErrConflict) || a.calls != 0 {
		t.Fatalf("stale binding %v calls=%d", err, a.calls)
	}
	if _, _, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, "bad", findingOne); !errors.Is(err, ErrInvalid) || a.calls != 0 {
		t.Fatalf("bad ID %v", err)
	}
}
func TestAuditVerificationNeverTreatsMissingFailureOrWrongPageAsPass(t *testing.T) {
	for _, change := range []func(*auditStub){
		func(a *auditStub) { a.err = errors.New("capture failed") },
		func(a *auditStub) { a.check = siteaudit.CheckEvidence{} },
		func(a *auditStub) { a.check.Status = "missing" },
		func(a *auditStub) { a.check.PageURL = "https://triodelab.no/other" },
		func(a *auditStub) { a.check.RuleID = "http_error" },
		func(a *auditStub) { a.check.AuditID = auditOne },
		func(a *auditStub) { a.check.ObservedAt = "2026-10-22T10:00:00Z" },
		func(a *auditStub) { a.check.ObservedAt = "2026-10-22T13:00:00Z" },
	} {
		s, _, w, a, _ := auditService(t)
		task := saveAudit(t, s, w).Tasks[0]
		change(a)
		if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1); err == nil {
			t.Fatal("missing/invalid evidence recorded")
		}
		value, _ := s.List(w.ID, w.Site)
		if value.Tasks[0].Verification != nil || len(value.Activity) != 1 {
			t.Fatal("invalid check mutated queue")
		}
	}
}
func TestAuditSaveAndVerifyRecheckBindingAndRevision(t *testing.T) {
	s, registry, w, a, _ := auditService(t)
	task := saveAudit(t, s, w).Tasks[0]
	a.during = func() {
		_, err := s.Change(w.ID, w.Site, w.Revision, task.ID, 1, "in_progress")
		if err != nil {
			t.Error(err)
		}
	}
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1); !errors.Is(err, ErrConflict) {
		t.Fatalf("concurrent task %v", err)
	}
	a.during = func() {
		_, err := registry.Change(t.Context(), w.ID, "https://agenci.no/", w.Revision)
		if err != nil {
			t.Error(err)
		}
	}
	if _, _, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditOne, findingOne); !errors.Is(err, ErrConflict) {
		t.Fatalf("concurrent binding %v", err)
	}
}

func TestAuditVariantsRejectCorruptStateAndNeverAliasSnapshots(t *testing.T) {
	s, _, w, a, path := auditService(t)
	a.finding.RelatedURLs = []string{w.Site + "other"}
	value := saveAudit(t, s, w)
	task := value.Tasks[0]
	a.finding.RelatedURLs[0] = w.Site + "source-mutated"
	value.Tasks[0].Evidence.Audit.RelatedURLs[0] = w.Site + "view-mutated"
	value, _ = s.List(w.ID, w.Site)
	if value.Tasks[0].Evidence.Audit.RelatedURLs[0] != w.Site+"other" {
		t.Fatal("evidence pointer alias")
	}
	a.check.Status = "failed"
	a.check.FindingID = findingOne
	value, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1)
	if err != nil {
		t.Fatal(err)
	}
	value.Tasks[0].Verification.Audit.Fingerprint = "mutated"
	value.Activity[0].Verification.Audit.Fingerprint = "mutated"
	value, _ = s.List(w.ID, w.Site)
	if value.Tasks[0].Verification.Audit.Fingerprint != a.check.Fingerprint || value.Activity[0].Verification.Audit.Fingerprint != a.check.Fingerprint {
		t.Fatal("verification pointer alias")
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, bad := range []string{
		strings.Replace(string(raw), `"relatedUrls":["`+w.Site+`other"]`, `"relatedUrls":null`, 1),
		strings.Replace(string(raw), `"severity":"warning"`, `"severity":"critical"`, 1),
		strings.Replace(string(raw), `"auditId":"`+auditOne+`"`, `"auditId":"invalid"`, 1),
		strings.Replace(string(raw), `"key":"missing_title_signal"`, `"key":"unknown_rule"`, 1),
		strings.Replace(string(raw), `"severity":"warning"`, `"severity":"warning","clicks":0`, 1),
		strings.Replace(string(raw), `"status":"failed","observedAt"`, `"status":"passed","score":1,"observedAt"`, 1),
	} {
		if err := os.WriteFile(path, []byte(bad), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := Open(path, "org-one"); err == nil {
			t.Fatalf("corrupt audit accepted %s", bad)
		}
	}
	var evidence Evidence
	for _, raw := range []string{`{"kind":"site_audit_finding"}`, `{"kind":"site_audit_finding","key":1}`, `{"kind":"site_audit_finding","score":1}`} {
		if json.Unmarshal([]byte(raw), &evidence) == nil {
			t.Fatalf("invalid evidence decoded %s", raw)
		}
	}
	var verification Verification
	for _, raw := range []string{`{"auditId":"bad"}`, `{"auditId":"bad","score":1}`, `{"auditId":1}`, `{"unknown":1}`} {
		if json.Unmarshal([]byte(raw), &verification) == nil {
			t.Fatalf("invalid verification decoded %s", raw)
		}
	}
}

func TestAuditUsesLatestNewAuditAndRepositoryCommitIsAtomic(t *testing.T) {
	s, _, w, a, path := auditService(t)
	task := saveAudit(t, s, w).Tasks[0]
	a.check.Status = "review"
	a.check.FindingID = findingOne
	value, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1)
	if err != nil || value.Tasks[0].Verification.Status != "review" {
		t.Fatalf("review %v", err)
	}
	a.check.ObservedAt = "2026-10-22T11:30:00Z"
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 2); !errors.Is(err, ErrConflict) {
		t.Fatalf("same audit reinterpreted %v", err)
	}
	a.check.AuditID = "audit_33333333333333333333333333333333"
	s.repo.path = filepath.Join(filepath.Dir(path), "missing", "work.json")
	if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 2); err == nil {
		t.Fatal("failed persistence succeeded")
	}
	value, _ = s.List(w.ID, w.Site)
	if value.Tasks[0].Revision != 2 || len(value.Activity) != 2 || value.Tasks[0].Verification.Audit.AuditID != auditTwo {
		t.Fatal("failed commit changed in-memory state")
	}
}

func TestAuditSourceErrorsAndReadOnlyPrerequisites(t *testing.T) {
	s, _, w, a, _ := auditService(t)
	without := s.WithAudit(nil)
	if s.audit == nil {
		t.Fatal("WithAudit mutated original")
	}
	if _, _, err := without.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditOne, findingOne); !errors.Is(err, ErrSourceUnavailable) {
		t.Fatalf("no source %v", err)
	}
	task := saveAudit(t, s, w).Tasks[0]
	if _, err := without.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1); !errors.Is(err, ErrSourceUnavailable) {
		t.Fatalf("no verification source %v", err)
	}
	for _, pair := range []struct{ source, want error }{{siteaudit.ErrInvalid, ErrInvalid}, {siteaudit.ErrConflict, ErrConflict}, {siteaudit.ErrNotFound, ErrNotFound}, {siteaudit.ErrUnavailable, ErrSourceUnavailable}} {
		a.err = pair.source
		if _, _, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditOne, findingOne); !errors.Is(err, pair.want) {
			t.Fatalf("save error %v wanted %v", err, pair.want)
		}
		if _, err := s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, 1); !errors.Is(err, pair.want) {
			t.Fatalf("verify error %v wanted %v", err, pair.want)
		}
	}
}

func TestAuditDeduplicatesAcrossRunsByPageAndRuleNotFindingID(t *testing.T) {
	s, _, w, a, _ := auditService(t)
	saveAudit(t, s, w)
	a.finding.ID = "finding_22222222222222222222222222222222"
	value, created, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditTwo, a.finding.ID)
	if err != nil || created || len(value.Tasks) != 1 || len(value.Activity) != 1 {
		t.Fatalf("run duplicate %+v %v %v", value, created, err)
	}
	a.finding.PageURL = w.Site + "other"
	value, created, err = s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditTwo, a.finding.ID)
	if err != nil || !created || len(value.Tasks) != 2 {
		t.Fatalf("distinct page %+v %v %v", value, created, err)
	}
}

func TestAuditFailedSaveCommitLeavesQueueAndDiskUnchanged(t *testing.T) {
	s, _, w, _, path := auditService(t)
	original, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	s.repo.path = filepath.Join(filepath.Dir(path), "missing", "work.json")
	if _, _, err := s.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, auditOne, findingOne); err == nil {
		t.Fatal("failed persistence succeeded")
	}
	value, _ := s.List(w.ID, w.Site)
	if len(value.Tasks) != 0 || len(value.Activity) != 0 {
		t.Fatal("failed commit changed queue")
	}
	after, err := os.ReadFile(path)
	if err != nil || string(original) != string(after) {
		t.Fatal("failed commit changed durable state")
	}
}
