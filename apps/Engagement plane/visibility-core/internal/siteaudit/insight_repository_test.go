package siteaudit

import (
	"encoding/json"
	"errors"
	"os"
	"strings"
	"testing"
	"time"
)

func TestInsightUpgradeBackfillsOnlyRetainedObservationsOnce(t *testing.T) {
	s, r, _, file := testService(t, source())
	for _, v := range []Snapshot{insightRun(t, s, 10, ""), insightRun(t, s, 11, "Fixed")} {
		saveInsight(t, r, v)
	}
	legacy := cloneState(r.state)
	legacy.InsightVersion, legacy.Insights = 0, nil
	raw, _ := json.Marshal(legacy)
	if err := os.WriteFile(file, raw, 0600); err != nil {
		t.Fatal(err)
	}
	for range 2 {
		restored, err := Open(file, "org-one")
		if err != nil {
			t.Fatal(err)
		}
		s.repo = restored
		v, _ := s.Insights(testID, testSite)
		w, _ := s.Weekly(testID, testSite, "2026-10-19")
		if !v.HistoryLimited || v.TrackingSince != "2026-10-22T10:00:00Z" || w.Audits.Total != 2 || w.Changes.Resolved != 1 {
			t.Fatal("upgrade invented/lost/recounted history", v, w)
		}
	}
}
func TestInsightWeeklyReceiptCapKeepsExactRollupAndRestart(t *testing.T) {
	s, r, _, file := testService(t, source())
	start := time.Date(2026, 10, 19, 10, 0, 0, 0, time.UTC)
	for i := range 25 {
		v := insightRun(t, s, 10, "")
		at := start.Add(time.Duration(i) * time.Minute)
		v.StartedAt, v.CompletedAt = stamp(at), stamp(at.Add(time.Second))
		v.Pages[0].ObservedAt = v.StartedAt
		v.Pages, v.Findings = evaluate(v.Pages)
		saveInsight(t, r, v)
	}
	restored, err := Open(file, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	s.repo = restored
	w, _ := s.Weekly(testID, testSite, "2026-10-19")
	if w.Audits.Total != 25 || len(w.Receipts) != maxWeeklyReceipts || !w.Truncated || w.Pages.Checked != 25 || w.FailedChecks != 25 {
		t.Fatal("receipt eviction changed measurements", w)
	}
}
func TestInsightFailedPersistenceCannotMutateNestedJournal(t *testing.T) {
	s, r, _, _ := testService(t, source())
	saveInsight(t, r, insightRun(t, s, 10, ""))
	before, _ := json.Marshal(r.state)
	r.beforePersist = func(diskState) error { return errors.New("unavailable") }
	if err := r.save(insightRun(t, s, 11, "Fixed")); err == nil {
		t.Fatal("expected failure")
	}
	after, _ := json.Marshal(r.state)
	if string(before) != string(after) {
		t.Fatal("failed commit mutated previous journal")
	}
	r.beforePersist = nil
	saveInsight(t, r, insightRun(t, s, 12, "Fixed"))
	w, _ := s.Weekly(testID, testSite, "2026-10-19")
	if w.Audits.Total != 2 {
		t.Fatal("uncommitted run counted", w)
	}
}
func TestInsightByteBudgetPrunesInBatchesAndMarksUnknownHistory(t *testing.T) {
	s, r, _, _ := testService(t, source())
	saveInsight(t, r, insightRun(t, s, 10, ""))
	state := cloneState(r.state)
	record := state.Insights[0]
	base := record.Conditions[0]
	base.SeenIssue, base.Unresolved, base.Status, base.CheckStatus = true, true, "unchanged", "failed"
	urls := []string{}
	for i := range maxLinks {
		urls = append(urls, testSite+strings.Repeat("a", 400)+string(rune('a'+i%26))+string(rune('a'+i/26)))
	}
	record.Conditions = []conditionRecord{}
	for i := range maxFileBytes/(len(urls[0])*maxLinks*2) + 10 {
		v := cloneCondition(base)
		v.PageURL = testSite + strings.Repeat("b", i+1)
		v.FinalURL = v.PageURL
		v.RelatedURLs = append([]string{}, urls...)
		v.PendingDependencies = append([]string{}, urls...)
		record.Conditions = append(record.Conditions, v)
	}
	state.Insights = []insightRecord{record}
	raw, _ := json.Marshal(state)
	if len(raw) <= maxFileBytes {
		t.Fatal("test did not exceed real byte cap")
	}
	fitted, err := fitInsightBudget(state, state.Runs[0].ID)
	if err != nil {
		t.Fatal(err)
	}
	raw, _ = json.Marshal(fitted)
	if len(raw) > maxFileBytes || !fitted.Insights[0].Truncated || !fitted.Insights[0].HistoryLimited || fitted.Insights[0].ComparisonAvailable || len(fitted.Runs) != 1 {
		t.Fatal("byte pruning lost authority/flags", len(raw))
	}
	if err := validateInsights(fitted.InsightVersion, fitted.Insights); err != nil {
		t.Fatal("invalid compact fallback", err)
	}
	// A single protected current report that cannot itself fit is rejected;
	// compact-history pruning cannot weaken the fixed two-MiB storage cap.
	state.Insights = []insightRecord{}
	state.Runs[0].Pages[0].Title = strings.Repeat("x", maxFileBytes+1)
	if _, err = fitInsightBudget(state, state.Runs[0].ID); !errors.Is(err, ErrCapacity) {
		t.Fatal("oversize current report accepted", err)
	}
}
func TestInsightStateValidationRejectsCorruptClosedFacts(t *testing.T) {
	s, r, _, _ := testService(t, source())
	saveInsight(t, r, insightRun(t, s, 10, ""))
	mutations := []func(*insightRecord){
		func(v *insightRecord) { v.WebsiteID = "bad" }, func(v *insightRecord) { v.Site = "https://other.no/" },
		func(v *insightRecord) { v.WebsiteRevision = 0 }, func(v *insightRecord) { v.RuleVersion = "forged" },
		func(v *insightRecord) { v.TrackingSince = "bad" }, func(v *insightRecord) { v.LatestCompletedAt = "bad" },
		func(v *insightRecord) { v.LatestStatus = "idle" }, func(v *insightRecord) { v.LatestReason = "token=secret" },
		func(v *insightRecord) { v.Processed = []string{} }, func(v *insightRecord) { v.Processed = []string{v.LatestAuditID, v.LatestAuditID} },
		func(v *insightRecord) { v.Conditions[0].RuleID = "forged" }, func(v *insightRecord) { v.Conditions[0].Severity = "critical" },
		func(v *insightRecord) { v.Conditions[0].Fingerprint = "fake" }, func(v *insightRecord) { v.Conditions[0].RelatedURLs = []string{"https://evil.no/"} },
		func(v *insightRecord) { v.Conditions[0].PendingDependencies = []string{"https://evil.no/"} },
		func(v *insightRecord) { v.Conditions[0].Status = "fake" }, func(v *insightRecord) { v.Conditions[0].CheckStatus = "fake" },
		func(v *insightRecord) { v.Weeks[0].WeekStart = "2026-10-20" }, func(v *insightRecord) { v.Weeks[0].Changes.New = -1 },
		func(v *insightRecord) { v.Weeks[0].Audits.Total = 10 }, func(v *insightRecord) { v.Weeks[0].Pages.Checked = 100 },
		func(v *insightRecord) { v.Weeks[0].Receipts[0].CompletedAt = "2026-11-01T10:00:00Z" },
		func(v *insightRecord) { v.Weeks = append(v.Weeks, v.Weeks[0]) }, func(v *insightRecord) { v.Conditions = append(v.Conditions, v.Conditions[0]) },
	}
	for i, mutate := range mutations {
		v := cloneInsight(r.state.Insights[0])
		mutate(&v)
		if validateInsights(1, []insightRecord{v}) == nil {
			t.Fatal("corrupt insight accepted", i)
		}
	}
	if validateInsights(9, []insightRecord{}) == nil || validateInsights(0, []insightRecord{}) == nil {
		t.Fatal("unknown journal format accepted")
	}
}
