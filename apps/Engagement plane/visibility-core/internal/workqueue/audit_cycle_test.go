package workqueue

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
)

// A controlled capture fixture exercises the real audit rules, persisted proof
// ledger and work queue together. It never injects a finding into live data.
type cycleCapture struct {
	phase atomic.Int32
	clock atomic.Int64
}

func (c *cycleCapture) Map(context.Context, string) ([]discovery.Page, error) {
	return []discovery.Page{{URL: "https://triodelab.no/"}}, nil
}
func (c *cycleCapture) Capture(_ context.Context, _, raw string) (siteaudit.CapturePage, error) {
	phase := c.phase.Load()
	if phase == 2 {
		return siteaudit.CapturePage{}, &siteaudit.CaptureError{Reason: "timeout"}
	}
	title := ""
	if phase == 1 {
		title = "Triodelab | Website solutions"
	}
	fingerprint := strings.Repeat("a", 64)
	if phase == 1 {
		fingerprint = strings.Repeat("b", 64)
	}
	return siteaudit.CapturePage{URL: raw, FinalURL: raw, HTTPStatus: 200, ContentType: "text/html", Title: title, Description: "Website design, technology and practical business outcomes.", Canonical: raw, Links: []string{}, ObservedAt: time.Unix(c.clock.Load(), 0).UTC().Format(time.RFC3339Nano), RunID: "run_01K7WKF1W4A0KWXY74XRXBZ6TT", Fingerprint: "blake3:" + fingerprint}, nil
}
func TestRealAuditRulesToTaskFixRecheckAndResolutionHistory(t *testing.T) {
	work, registry, w, _, workPath := testService(t)
	path := filepath.Join(filepath.Dir(workPath), "audits.json")
	auditsRepo, err := siteaudit.Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	capture := &cycleCapture{}
	capture.clock.Store(time.Date(2026, 10, 22, 12, 0, 0, 0, time.UTC).Unix())
	now := func() time.Time { return time.Unix(capture.clock.Load(), 0).UTC() }
	audits := siteaudit.New(auditsRepo, registry, capture, now)
	work = work.WithAudit(audits)
	work.now = now
	run := func(phase int32) siteaudit.Snapshot {
		t.Helper()
		capture.phase.Store(phase)
		capture.clock.Add(90)
		deadline := time.Now().Add(3 * time.Second)
		for {
			_, e := audits.Start(w.ID, w.Site, w.Revision)
			if e == nil {
				break
			}
			if !errors.Is(e, siteaudit.ErrCapacity) || time.Now().After(deadline) {
				t.Fatal(e)
			}
			time.Sleep(time.Millisecond)
		}
		for time.Now().Before(deadline) {
			v, e := audits.Get(w.ID, w.Site)
			if e != nil {
				t.Fatal(e)
			}
			if v.Status == "complete" || v.Status == "failed" {
				return v
			}
			time.Sleep(time.Millisecond)
		}
		t.Fatal("controlled audit did not finish")
		return siteaudit.Snapshot{}
	}
	broken := run(0)
	if len(broken.Findings) != 1 || broken.Findings[0].RuleID != "missing_title_signal" {
		t.Fatal("real rule did not identify missing title", broken)
	}
	saved, created, err := work.SaveAudit(t.Context(), w.ID, w.Site, w.Revision, broken.ID, broken.Findings[0].ID)
	if err != nil || !created {
		t.Fatal(err)
	}
	task := saved.Tasks[0]
	if task.Evidence.Audit.Severity != "warning" {
		t.Fatal("task lost finding priority")
	}
	saved, err = work.Change(w.ID, w.Site, w.Revision, task.ID, task.Revision, "in_progress")
	if err != nil {
		t.Fatal(err)
	}
	fixed := run(1)
	if len(fixed.Findings) != 0 {
		t.Fatal("fixture fix did not pass")
	}
	saved, err = work.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, saved.Tasks[0].Revision)
	if err != nil || saved.Tasks[0].Verification.Status != "passed" || saved.Tasks[0].Verification.Audit.AuditID != fixed.ID {
		t.Fatal("new passing evidence not persisted", err)
	}
	passed := saved.Tasks[0].Verification.ObservedAt
	run(2)
	if _, err = work.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, saved.Tasks[0].Revision); err == nil {
		t.Fatal("skipped/failed capture treated as passing proof")
	}
	run(3)
	saved, err = work.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, saved.Tasks[0].Revision)
	if err != nil || saved.Tasks[0].Verification.Status != "failed" {
		t.Fatal("regression not recorded", err)
	}
	repo, err := Open(workPath, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	restored, err := New(repo, registry, nil, now).List(w.ID, w.Site)
	if err != nil {
		t.Fatal(err)
	}
	seenPass, seenFailure := false, false
	for _, event := range restored.Activity {
		if event.Verification != nil {
			seenPass = seenPass || event.Verification.Status == "passed" && event.Verification.ObservedAt == passed
			seenFailure = seenFailure || event.Verification.Status == "failed"
		}
	}
	if !seenPass || !seenFailure || len(restored.Tasks) != 1 {
		t.Fatal("resolution/regression history lost")
	}
	info, _ := os.Stat(path)
	if info.Mode().Perm() != 0600 {
		t.Fatal("audit proof storage unprotected")
	}
}
