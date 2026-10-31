package siteaudit

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestRepositoryRejectsForeignCorruptUnprotectedHistory(t *testing.T) {
	s, _, _, file := testService(t, source())
	if _, err := s.Start(testID, testSite, 1); err != nil {
		t.Fatal(err)
	}
	finish(t, s)
	if _, err := Open(file, "other"); err == nil {
		t.Fatal("foreign organization accepted")
	}
	if _, err := Open("relative.json", "org-one"); err == nil {
		t.Fatal("relative store")
	}
	raw, err := os.ReadFile(file)
	if err != nil {
		t.Fatal(err)
	}
	for _, broken := range []string{string(raw) + "{}", strings.Replace(string(raw), `"ruleId":"missing_title_signal"`, `"ruleId":"forged_rule"`, 1), strings.Replace(string(raw), `"websiteRevision":1`, `"websiteRevision":0`, 1), strings.Replace(string(raw), `"status":"complete"`, `"status":"failed"`, 1), `{"version":1,"organizationId":"org-one","runs":null}`, strings.Repeat("x", maxFileBytes+1)} {
		if err := os.WriteFile(file, []byte(broken), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := Open(file, "org-one"); err == nil {
			t.Fatal("invalid history accepted")
		}
	}
	_ = os.WriteFile(file, raw, 0600)
	_ = os.Chmod(file, 0644)
	if _, err := Open(file, "org-one"); err == nil {
		t.Fatal("public store")
	}
	_ = os.Chmod(file, 0600)
	link := filepath.Join(filepath.Dir(file), "link.json")
	_ = os.Symlink(file, link)
	if _, err := Open(link, "org-one"); err == nil {
		t.Fatal("symlink store")
	}
	_ = os.Chmod(filepath.Dir(file), 0755)
	if _, err := Open(file, "org-one"); err == nil {
		t.Fatal("public directory")
	}
}
func TestRepositoryBoundsHistoryAndRecoversInterruptedRuns(t *testing.T) {
	s, repo, _, file := testService(t, source())
	v := s.emptyWebsiteFixture()
	v.ID = newID("audit_")
	v.Status = "running"
	v.StartedAt = "2026-10-22T10:00:00Z"
	if err := repo.save(v); err != nil {
		t.Fatal(err)
	}
	restored, err := Open(file, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	runs := restored.list(testID, testSite)
	if len(runs) != 1 || runs[0].Status != "failed" || runs[0].Reason != "interrupted" {
		t.Fatal("lost interrupted receipt", runs)
	}
	for range 30 {
		v.ID = newID("audit_")
		v.Status = "failed"
		v.Reason = "source_unavailable"
		v.CompletedAt = v.StartedAt
		if err := restored.save(v); err != nil {
			t.Fatal(err)
		}
	}
	if len(restored.list(testID, testSite)) != maxSiteRuns {
		t.Fatal("unbounded site history")
	}
	for i := range 25 {
		value := copySnapshot(v)
		value.WebsiteID = "web_" + strings.Repeat("a", 30) + string("abcdef"[i%6]) + string("0123456789"[i/6])
		value.ID = newID("audit_")
		if err := restored.save(value); err != nil {
			t.Fatal(err)
		}
	}
	if len(restored.state.Runs) != maxRuns {
		t.Fatal("unbounded organization history")
	}
	raw, _ := json.Marshal(restored.state)
	if len(raw) > maxFileBytes {
		t.Fatal("unbounded state")
	}
}
func (s *Service) emptyWebsiteFixture() Snapshot {
	w, _ := s.binding(testID, testSite, 1)
	return s.empty(w)
}

func TestDirectorySyncFailureKeepsMemoryConsistentWithCommittedDisk(t *testing.T) {
	s, repo, _, file := testService(t, source())
	v := s.emptyWebsiteFixture()
	v.ID = newID("audit_")
	v.Status = "failed"
	v.Reason = "source_unavailable"
	v.StartedAt = "2026-10-22T10:00:00Z"
	v.CompletedAt = v.StartedAt
	repo.syncDir = func(string) error { return errors.New("simulated post-rename directory sync failure") }
	if err := repo.save(v); !errors.Is(err, errCommitted) {
		t.Fatal("committed ambiguity not reported", err)
	}
	if len(repo.list(testID, testSite)) != 1 {
		t.Fatal("committed state lost from memory")
	}
	restored, err := Open(file, "org-one")
	if err != nil || len(restored.list(testID, testSite)) != 1 {
		t.Fatal("committed bytes missing", err)
	}
	repo.syncDir = nil
	newer := copySnapshot(v)
	newer.ID = newID("audit_")
	if err := repo.save(newer); err != nil {
		t.Fatal(err)
	}
	restored, err = Open(file, "org-one")
	if err != nil || len(restored.list(testID, testSite)) != 2 {
		t.Fatal("later commit overwrote already-committed audit", err)
	}
}

func TestAmbiguousStartStillRunsAlreadyCommittedReceipt(t *testing.T) {
	s, repo, _, _ := testService(t, source())
	repo.syncDir = func(string) error { return errors.New("simulated post-rename failure") }
	if _, err := s.Start(testID, testSite, 1); !errors.Is(err, errCommitted) {
		t.Fatal("start ambiguity not reported", err)
	}
	v := finish(t, s)
	if v.Status != "complete" {
		t.Fatal("already-committed run was stranded", v.Status)
	}
}
