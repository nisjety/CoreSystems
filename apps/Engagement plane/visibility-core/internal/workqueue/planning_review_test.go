package workqueue

import (
	"context"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/searchresearch"
	"testing"
)

func TestPlanningUsesRevisionAndPersistsIndependentHistory(t *testing.T) {
	s, registry, w, _, path := testService(t)
	saved := saveFinding(t, s, w)
	task := saved.Tasks[0]
	p := Planning{AssigneeID: "user-one", DueDate: "2026-11-02", Priority: "high"}
	v, err := s.Plan(w.ID, w.Site, w.Revision, task.ID, task.Revision, p)
	if err != nil || v.Tasks[0].Revision != 2 || v.Activity[0].Kind != "planning_changed" {
		t.Fatal(v, err)
	}
	if _, err = s.Plan(w.ID, w.Site, w.Revision, task.ID, 1, p); !errors.Is(err, ErrConflict) {
		t.Fatal(err)
	}
	v.Tasks[0].Planning.Priority = "low"
	again, _ := s.List(w.ID, w.Site)
	if again.Tasks[0].Planning.Priority != "high" {
		t.Fatal("mutable planning pointer escaped")
	}
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	v, _ = New(repo, registry, nil, nil).List(w.ID, w.Site)
	if v.Tasks[0].Planning.DueDate != p.DueDate {
		t.Fatal(v)
	}
	for _, bad := range []Planning{{DueDate: "2026-02-30", Priority: "high"}, {Priority: "invented"}, {AssigneeID: "../other", Priority: "normal"}} {
		if validPlanning(&bad) {
			t.Fatal(bad)
		}
	}
}

type researchStub struct{ value searchresearch.Snapshot }

func (r *researchStub) Get(context.Context, string, string) (searchresearch.Snapshot, error) {
	return r.value, nil
}
func TestSearchGroupRecheckNeedsNewObservedRowAndRetainsOriginalRule(t *testing.T) {
	s, _, w, _, path := testService(t)
	rule := searchresearch.Rule{ID: "review", Name: "Low visibility", Entity: "query", MinImpressions: 10, MaxPosition: 20}
	row := searchresearch.Aggregate{Key: "webdesign", Clicks: 1, Impressions: 100, Position: 15, Days: 3}
	source := &researchStub{searchresearch.Snapshot{WebsiteID: w.ID, Site: w.Site, WebsiteRevision: w.Revision, Status: "connected", ObservedAt: "2026-10-22T10:00:00Z", Queries: []searchresearch.Aggregate{row}, Groups: []searchresearch.Group{{Rule: rule, Items: []searchresearch.Aggregate{row}}}}}
	source.value.Period.StartDate = "2026-07-22"
	source.value.Period.EndDate = "2026-10-19"
	s = s.WithResearch(source)
	v, created, err := s.SaveReview(t.Context(), w.ID, w.Site, w.Revision, rule.ID, row.Key)
	if err != nil || !created {
		t.Fatal(v, err)
	}
	task := v.Tasks[0]
	if _, err = s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, task.Revision); !errors.Is(err, ErrConflict) {
		t.Fatal("old report verified", err)
	}
	source.value.ObservedAt = "2026-10-22T11:00:00Z"
	source.value.Queries = nil
	v, err = s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, task.Revision)
	if err != nil || v.Tasks[0].Verification.Status != "inconclusive" || v.Tasks[0].Status != "todo" {
		t.Fatal(v, err)
	}
	source.value.ObservedAt = "2026-10-22T11:30:00Z"
	row.Position = 25
	source.value.Queries = []searchresearch.Aggregate{row}
	task = v.Tasks[0]
	v, err = s.Verify(t.Context(), w.ID, w.Site, w.Revision, task.ID, task.Revision)
	if err != nil || v.Tasks[0].Verification.Status != "passed" || v.Tasks[0].Status != "todo" {
		t.Fatal(v, err)
	}
	if _, err = Open(path, "org-one"); err != nil {
		t.Fatal(err)
	}
}
