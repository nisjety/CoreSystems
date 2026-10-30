package siteaudit

import (
	"errors"
	"testing"
	"time"
)

func insightRun(t *testing.T, s *Service, hour int, title string) Snapshot {
	t.Helper()
	record, _ := s.registry.Resolve(testID, testSite)
	v := s.empty(record.Website)
	v.ID, v.Status = newID("audit_"), "complete"
	v.StartedAt = stamp(time.Date(2026, 10, 22, hour, 0, 0, 0, time.UTC))
	v.CompletedAt = stamp(mustTime(v.StartedAt).Add(time.Second))
	v.Pages = []Page{{URL: testSite, FinalURL: testSite, Status: "checked", HTTPStatus: 200, Title: title, Description: "Description", Canonical: testSite, ObservedAt: v.StartedAt, RunID: "run_01K7WKF1W4A0KWXY74XRXBZ6TT", Fingerprint: "blake3:" + repeatA(), Links: []string{}, Checks: []Check{}}}
	v.Pages, v.Findings = evaluate(v.Pages)
	return v
}
func repeatA() string { return "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }

func TestInsightsRequireNewExplicitEvidenceAndRememberRegression(t *testing.T) {
	s, repo, _, file := testService(t, source())
	first := insightRun(t, s, 10, "")
	if err := repo.save(first); err != nil {
		t.Fatal(err)
	}
	v, err := s.Insights(testID, testSite)
	if err != nil || v.ComparisonAvailable || v.Summary.New != 1 {
		t.Fatalf("baseline %#v %v", v, err)
	}
	failed := insightRun(t, s, 11, "")
	failed.Status, failed.Reason, failed.Pages, failed.Findings = "failed", "timeout", []Page{}, []Finding{}
	if err := repo.save(failed); err != nil {
		t.Fatal(err)
	}
	v, _ = s.Insights(testID, testSite)
	if v.Summary.Unknown != 1 || len(v.Alerts) < 1 {
		t.Fatalf("failed newest must not resolve %#v", v)
	}
	fixed := insightRun(t, s, 12, "Fixed")
	if err := repo.save(fixed); err != nil {
		t.Fatal(err)
	}
	v, _ = s.Insights(testID, testSite)
	if v.Summary.Resolved != 1 {
		t.Fatalf("explicit resolution %#v", v)
	}
	regressed := insightRun(t, s, 13, "")
	if err := repo.save(regressed); err != nil {
		t.Fatal(err)
	}
	v, _ = s.Insights(testID, testSite)
	if v.Summary.Regressed != 1 {
		t.Fatalf("regression %#v", v)
	}
	if err := repo.save(regressed); err != nil {
		t.Fatal(err)
	}
	w, err := s.Weekly(testID, testSite, "2026-10-19")
	if err != nil || w.Audits.Total != 4 || w.Changes.Resolved != 1 || w.Changes.Regressed != 1 {
		t.Fatalf("idempotence %#v %v", w, err)
	}
	restored, err := Open(file, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	s.repo = restored
	w, err = s.Weekly(testID, testSite, "2026-10-19")
	if err != nil || w.Audits.Total != 4 {
		t.Fatal("restart lost compact journal", w, err)
	}
}

func TestInsightsNeverResolveSkippedDependency(t *testing.T) {
	s, repo, _, _ := testService(t, source())
	first := insightRun(t, s, 10, "Duplicated")
	partner := first.Pages[0]
	partner.URL, partner.FinalURL = testSite+"partner", testSite+"partner"
	partner.Canonical = partner.FinalURL
	first.Pages = append(first.Pages, partner)
	first.Pages, first.Findings = evaluate(first.Pages)
	if err := repo.save(first); err != nil {
		t.Fatal(err)
	}
	newer := insightRun(t, s, 11, "Duplicated")
	other := newer.Pages[0]
	other.URL, other.FinalURL, other.Title = testSite+"other", testSite+"other", "Different"
	other.Canonical = other.FinalURL
	newer.Pages = append(newer.Pages, other)
	newer.Pages, newer.Findings = evaluate(newer.Pages)
	if err := repo.save(newer); err != nil {
		t.Fatal(err)
	}
	v, _ := s.Insights(testID, testSite)
	if v.Summary.Resolved != 0 || v.Summary.Unknown != 2 {
		t.Fatalf("skipped duplicate partners treated resolved %#v", v)
	}
}

func TestInsightsAmbiguousCommitDoesNotDoubleCount(t *testing.T) {
	s, repo, _, _ := testService(t, source())
	v := insightRun(t, s, 10, "")
	repo.syncDir = func(string) error { return errors.New("post rename") }
	if err := repo.save(v); !errors.Is(err, errCommitted) {
		t.Fatal(err)
	}
	repo.syncDir = nil
	if err := repo.save(v); err != nil {
		t.Fatal(err)
	}
	w, err := s.Weekly(testID, testSite, "2026-10-19")
	if err != nil || w.Audits.Total != 1 {
		t.Fatal("double counted", w, err)
	}
}

func TestInsightsKeepDependenciesAcrossChangedFailedObservations(t *testing.T) {
	s, repo, _, _ := testService(t, source())
	makeRun := func(hour int, target string, code int) Snapshot {
		v := insightRun(t, s, hour, "Title")
		v.Pages[0].Links = []string{testSite + target}
		p := v.Pages[0]
		p.URL, p.FinalURL, p.Canonical = testSite+target, testSite+target, testSite+target
		p.HTTPStatus = code
		p.Links = []string{}
		v.Pages = append(v.Pages, p)
		v.Pages, v.Findings = evaluate(v.Pages)
		return v
	}
	for _, v := range []Snapshot{makeRun(10, "b", 404), makeRun(11, "c", 404), makeRun(12, "c", 200)} {
		if err := repo.save(v); err != nil {
			t.Fatal(err)
		}
	}
	v, _ := s.Insights(testID, testSite)
	for _, item := range v.Items {
		if item.PageURL == testSite && item.RuleID == "broken_checked_link" && item.Status != "unknown" {
			t.Fatal("unchecked former target forgotten", item)
		}
	}
	check, err := s.LatestCheck(testID, testSite, 1, testSite, "broken_checked_link")
	if err != nil || check.Status != "inconclusive" {
		t.Fatal("work verification disagreed with unknown comparison", check, err)
	}
}
