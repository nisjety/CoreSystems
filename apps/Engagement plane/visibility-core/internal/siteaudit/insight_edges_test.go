package siteaudit

import (
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"
)

func saveInsight(t *testing.T, r *Repository, v Snapshot) {
	t.Helper()
	if err := r.save(v); err != nil {
		t.Fatal(err)
	}
}
func TestInsightsFirstIssueIsNewNotRegressedAndFailedBaselineIsNotComparison(t *testing.T) {
	s, r, _, _ := testService(t, source())
	failed := insightRun(t, s, 9, "")
	failed.Status, failed.Reason, failed.Pages, failed.Findings = "failed", "timeout", []Page{}, []Finding{}
	saveInsight(t, r, failed)
	saveInsight(t, r, insightRun(t, s, 10, "Healthy"))
	v, _ := s.Insights(testID, testSite)
	if v.ComparisonAvailable || len(v.Items) != 0 {
		t.Fatal("failed first run is not a measured baseline", v)
	}
	saveInsight(t, r, insightRun(t, s, 11, ""))
	v, _ = s.Insights(testID, testSite)
	if v.Summary.New != 1 || v.Summary.Regressed != 0 {
		t.Fatal("first ever issue called regression", v)
	}
}
func TestInsightsUnknownForStaleCaptureMovedFinalAndMissingCanonical(t *testing.T) {
	for _, name := range []string{"stale", "final", "canonical"} {
		t.Run(name, func(t *testing.T) {
			s, r, _, _ := testService(t, source())
			first := insightRun(t, s, 10, "")
			if name == "canonical" {
				first.Pages[0].Title = "Title"
				first.Pages[0].Canonical = testSite + "elsewhere"
				first.Pages, first.Findings = evaluate(first.Pages)
			}
			if name == "stale" {
				first.Pages[0].ObservedAt = "2026-10-22T11:00:00Z"
				first.CompletedAt = "2026-10-22T11:00:01Z"
				first.Pages, first.Findings = evaluate(first.Pages)
			}
			saveInsight(t, r, first)
			next := insightRun(t, s, 11, "Fixed")
			if name == "final" {
				next.Pages[0].FinalURL = "https://www.triodelab.no/"
				next.Pages[0].Canonical = next.Pages[0].FinalURL
			}
			if name == "canonical" {
				next.Pages[0].Canonical = ""
			}
			next.Pages, next.Findings = evaluate(next.Pages)
			saveInsight(t, r, next)
			v, _ := s.Insights(testID, testSite)
			if v.Summary.Resolved != 0 || v.Summary.Unknown != 1 {
				t.Fatal("unproven resolution", name, v)
			}
		})
	}
}
func TestInsightsRunningAndRevisionChangesHaveNoStaleSuccess(t *testing.T) {
	s, r, registry, _ := testService(t, source())
	saveInsight(t, r, insightRun(t, s, 10, ""))
	running := insightRun(t, s, 11, "")
	running.Status, running.CompletedAt, running.Pages, running.Findings = "running", "", []Page{}, []Finding{}
	saveInsight(t, r, running)
	v, _ := s.Insights(testID, testSite)
	if v.LatestStatus != "running" || v.Summary.Unknown != 1 || v.Summary.Resolved != 0 {
		t.Fatal("running retained old success", v)
	}
	registry.mu.Lock()
	registry.revision = 2
	registry.mu.Unlock()
	v, _ = s.Insights(testID, testSite)
	if len(v.Items) != 0 || v.TrackingSince != "" || v.WebsiteRevision != 2 {
		t.Fatal("cross revision history leaked", v)
	}
	newContext := insightRun(t, s, 12, "Fixed")
	newContext.WebsiteRevision = 2
	saveInsight(t, r, newContext)
	oldCompletion := insightRun(t, s, 11, "")
	oldCompletion.WebsiteRevision = 1
	oldCompletion.ID = running.ID
	oldCompletion.CompletedAt = "2026-10-22T12:30:00Z"
	saveInsight(t, r, oldCompletion)
	v, _ = s.Insights(testID, testSite)
	if len(v.Items) != 0 || v.LatestAuditID != newContext.ID {
		t.Fatal("late old context overwrote new", v)
	}
}
func TestInsightsLateClaimedAttemptCountsButDoesNotReplaceNewerProof(t *testing.T) {
	s, r, _, _ := testService(t, source())
	older := insightRun(t, s, 10, "")
	running := copySnapshot(older)
	running.Status, running.CompletedAt, running.Pages, running.Findings = "running", "", []Page{}, []Finding{}
	saveInsight(t, r, running)
	newer := insightRun(t, s, 11, "Healthy")
	saveInsight(t, r, newer)
	older.CompletedAt = "2026-10-22T12:00:00Z"
	saveInsight(t, r, older)
	saveInsight(t, r, older)
	v, _ := s.Insights(testID, testSite)
	w, _ := s.Weekly(testID, testSite, "2026-10-19")
	if v.LatestAuditID != newer.ID || len(v.Items) != 0 || w.Audits.Total != 2 {
		t.Fatal("late receipt lost/overwrote/doubled", v, w)
	}
}
func TestInsightsBoundConditionsAndDependencyOverflowStayUnknown(t *testing.T) {
	s, r, _, _ := testService(t, source())
	for i := range 4 {
		v := insightRun(t, s, 9+i, "")
		base := v.Pages[0]
		v.Pages = []Page{}
		for j := range Limit {
			p := base
			p.URL = fmt.Sprintf("%srun-%d-page-%d", testSite, i, j)
			p.FinalURL, p.Canonical = p.URL, p.URL
			v.Pages = append(v.Pages, p)
		}
		v.Pages, v.Findings = evaluate(v.Pages)
		saveInsight(t, r, v)
	}
	v, _ := s.Insights(testID, testSite)
	if !v.Truncated || v.ComparisonAvailable || len(v.Items) > maxConditions {
		t.Fatal("condition cap not explicit", v)
	}
	previous := conditionRecord{InsightItem: InsightItem{RuleID: "broken_checked_link"}, DependencyLimited: false, PendingDependencies: []string{}}
	next := conditionRecord{InsightItem: InsightItem{RuleID: "broken_checked_link", RelatedURLs: []string{testSite + "new"}}}
	for i := range maxLinks {
		previous.PendingDependencies = append(previous.PendingDependencies, testSite+strings.Repeat("a", i+1))
	}
	deps, limited := mergeDependencies(previous, next)
	if !limited || len(deps) != maxLinks {
		t.Fatal("dependency union unbounded", len(deps), limited)
	}
}
func TestWeeklyUTCPeriodsRetainObservedCountsBeyondFullReportEviction(t *testing.T) {
	s, r, _, _ := testService(t, source())
	base := time.Date(2026, 8, 3, 23, 59, 0, 0, time.UTC)
	for i := range 10 {
		v := insightRun(t, s, 10, "")
		start := base.AddDate(0, 0, i*7)
		v.StartedAt, v.CompletedAt = stamp(start), stamp(start.Add(time.Second))
		v.Pages[0].ObservedAt = v.StartedAt
		v.Pages, v.Findings = evaluate(v.Pages)
		saveInsight(t, r, v)
	}
	s.now = func() time.Time { return time.Date(2026, 10, 22, 12, 0, 0, 0, time.UTC) }
	w, err := s.Weekly(testID, testSite, "2026-09-28")
	if err != nil || w.Audits.Total != 1 || !w.Observed || w.Status != "closed" || !w.HistoryLimited || len(w.AvailableWeeks) != 8 || len(r.list(testID, testSite)) != 5 {
		t.Fatal("weekly ledger depended on full reports", w, err)
	}
	for _, week := range []string{"2026-10-20", "2026-10-26", "2026-02-30"} {
		if _, err = s.Weekly(testID, testSite, week); err == nil {
			t.Fatal("invalid/future week", week)
		}
	}
	empty, _ := s.Weekly(testID, testSite, "")
	if empty.Observed || empty.Status != "open" {
		t.Fatal("empty week fabricated measurements", empty)
	}
}

func TestInsightsDuplicateDependenciesAndPositiveTitlesAcrossThreeAudits(t *testing.T) {
	for _, scenario := range []string{"changed partner", "empty partner title"} {
		t.Run(scenario, func(t *testing.T) {
			s, r, _, _ := testService(t, source())
			makeRun := func(hour int, partner, title string) Snapshot {
				v := insightRun(t, s, hour, "Same")
				p := v.Pages[0]
				p.URL, p.FinalURL, p.Canonical = testSite+partner, testSite+partner, testSite+partner
				p.Title = title
				v.Pages = append(v.Pages, p)
				v.Pages, v.Findings = evaluate(v.Pages)
				return v
			}
			saveInsight(t, r, makeRun(10, "b", "Same"))
			if scenario == "changed partner" {
				saveInsight(t, r, makeRun(11, "c", "Same"))
				saveInsight(t, r, makeRun(12, "c", "Different"))
			} else {
				saveInsight(t, r, makeRun(11, "b", ""))
			}
			v, _ := s.Insights(testID, testSite)
			found := false
			for _, item := range v.Items {
				if item.PageURL == testSite && item.RuleID == "duplicate_title_signal" {
					found = true
					if item.Status != "unknown" {
						t.Fatal("dependency disappearance/empty title resolved duplicate", scenario, item)
					}
				}
			}
			if !found {
				t.Fatal("lost unresolved duplicate condition", v)
			}
		})
	}
}

func TestLatestWorkCheckUsesAtomicCurrentReceiptAndKnownIssue(t *testing.T) {
	s, r, _, _ := testService(t, source())
	saveInsight(t, r, insightRun(t, s, 10, "Healthy"))
	check, err := s.LatestCheck(testID, testSite, 1, testSite, "missing_title_signal")
	if err != nil || check.Status != "inconclusive" {
		t.Fatal("healthy baseline falsely verified unseen legacy issue", check, err)
	}
	running := insightRun(t, s, 11, "Healthy")
	running.Status, running.CompletedAt, running.Pages, running.Findings = "running", "", []Page{}, []Finding{}
	saveInsight(t, r, running)
	ledger, exists, current := r.insight(testID, testSite, 1)
	if !exists || current.ID != running.ID || ledger.LatestAuditID == current.ID {
		t.Fatal("inconsistent atomically read lifecycle", ledger, current)
	}
	if _, err = s.LatestCheck(testID, testSite, 1, testSite, "missing_title_signal"); !errors.Is(err, ErrUnavailable) {
		t.Fatal("fell back past new running audit", err)
	}
	// Returned records own their nested memory; later repository changes cannot
	// turn this coherent point-in-time read into a mixed old/new proof.
	ledger.Conditions[0].RelatedURLs = append(ledger.Conditions[0].RelatedURLs, testSite+"injected")
	copy, _, _ := r.insight(testID, testSite, 1)
	if len(copy.Conditions[0].RelatedURLs) != 0 {
		t.Fatal("atomic read leaked mutable repository state")
	}
}

func TestLatestTaskCheckRequiresKnownOriginalAuditNotLaterUnrelatedResolution(t *testing.T) {
	s, r, _, _ := testService(t, source())
	// A pre-feature A/B task is unknown; only a later A/C incident and its
	// resolution are present in this ledger. They cannot verify old B.
	makeRun := func(hour int, title string) Snapshot {
		v := insightRun(t, s, hour, title)
		p := v.Pages[0]
		p.URL, p.FinalURL, p.Canonical = testSite+"c", testSite+"c", testSite+"c"
		p.Title = "Same"
		v.Pages = append(v.Pages, p)
		v.Pages, v.Findings = evaluate(v.Pages)
		return v
	}
	known := makeRun(10, "Same")
	saveInsight(t, r, known)
	saveInsight(t, r, makeRun(11, "Different"))
	for _, tc := range []struct{ id, status string }{{"audit_11111111111111111111111111111111", "inconclusive"}, {known.ID, "passed"}} {
		check, err := s.LatestTaskCheck(testID, testSite, 1, tc.id, testSite, "duplicate_title_signal")
		if err != nil || check.Status != tc.status {
			t.Fatal("original audit authority lost", check, err)
		}
	}
	if _, err := s.LatestTaskCheck(testID, testSite, 1, "bad", testSite, "duplicate_title_signal"); !errors.Is(err, ErrInvalid) {
		t.Fatal("invalid original ID accepted", err)
	}
}

func TestPrunedConditionCannotVerifyOriginalTaskAfterFinalURLChanged(t *testing.T) {
	s, r, _, _ := testService(t, source())
	original := insightRun(t, s, 10, "")
	saveInsight(t, r, original)
	r.mu.Lock()
	record := cloneInsight(r.state.Insights[0])
	record.Conditions = []conditionRecord{}
	record.Truncated, record.HistoryLimited = true, true
	r.state.Insights[0] = record
	r.mu.Unlock()
	for _, v := range []Snapshot{insightRun(t, s, 11, ""), insightRun(t, s, 12, "Fixed")} {
		v.Pages[0].FinalURL, v.Pages[0].Canonical = "https://www.triodelab.no/", "https://www.triodelab.no/"
		v.Pages, v.Findings = evaluate(v.Pages)
		saveInsight(t, r, v)
	}
	v, _ := s.Insights(testID, testSite)
	if v.Summary.Resolved != 0 || v.Summary.Unknown != 1 {
		t.Fatal("lost finalURL continuity called resolved", v)
	}
	check, err := s.LatestTaskCheck(testID, testSite, 1, original.ID, testSite, "missing_title_signal")
	if err != nil || check.Status != "inconclusive" {
		t.Fatal("retained global source ID substituted for condition continuity", check, err)
	}
}
