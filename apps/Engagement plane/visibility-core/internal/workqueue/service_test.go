package workqueue

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type searchStub struct {
	projection pulse.SearchProjection
	calls      int
	during     func()
}

func (s *searchStub) GetSearch(context.Context, string, string) pulse.SearchProjection {
	s.calls++
	if s.during != nil {
		s.during()
	}
	return s.projection
}

func testService(t *testing.T) (*Service, *websites.Service, websites.Website, *searchStub, string) {
	t.Helper()
	dir := t.TempDir()
	if err := os.Chmod(dir, 0700); err != nil {
		t.Fatal(err)
	}
	registryRepo, err := websites.Open(filepath.Join(dir, "sites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	registry := websites.NewService(registryRepo, nil, nil)
	website, _, err := registry.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "work.json")
	repo, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	source := &searchStub{projection: pulse.SearchProjection{Site: website.Site, Status: pulse.StatusConnected, Source: "google_search_console", Period: pulse.Period{StartDate: "2026-10-01", EndDate: "2026-10-19"}, UpdatedAt: "2026-10-22T10:00:00Z", Opportunities: []pulse.Opportunity{{SearchRow: pulse.SearchRow{Key: "webdesign", Clicks: 2, Impressions: 200, CTR: 0.01, Position: 7}, Type: "title_meta"}}}}
	return New(repo, registry, source, func() time.Time { return time.Date(2026, 10, 22, 12, 0, 0, 0, time.UTC) }), registry, website, source, path
}
func saveFinding(t *testing.T, service *Service, w websites.Website) Snapshot {
	t.Helper()
	result, created, err := service.Save(t.Context(), w.ID, w.Site, w.Revision, "Q4", "webdesign")
	if err != nil || !created {
		t.Fatalf("save: created=%v err=%v", created, err)
	}
	return result
}

func TestFindingPersistsEvidenceDeduplicatesAndTracksLifecycle(t *testing.T) {
	s, registry, w, source, path := testService(t)
	first := saveFinding(t, s, w)
	if len(first.Tasks) != 1 || len(first.Activity) != 1 || first.Summary.Open != 1 || first.Summary.CompletionPercent == nil || *first.Summary.CompletionPercent != 0 {
		t.Fatalf("first=%+v", first)
	}
	task := first.Tasks[0]
	if task.Evidence.Key != "webdesign" || task.Evidence.Clicks != 2 || task.Evidence.ObservedAt != source.projection.UpdatedAt || task.Status != "todo" || task.Revision != 1 {
		t.Fatalf("task=%+v", task)
	}
	duplicate, created, err := s.Save(t.Context(), w.ID, w.Site, w.Revision, "Q4", "webdesign")
	if err != nil || created || len(duplicate.Tasks) != 1 || len(duplicate.Activity) != 1 {
		t.Fatalf("duplicate=%+v created=%v err=%v", duplicate, created, err)
	}
	next, err := s.Change(w.ID, w.Site, w.Revision, task.ID, 1, "in_progress")
	if err != nil || next.Summary.InProgress != 1 || next.Tasks[0].Revision != 2 {
		t.Fatalf("next=%+v err=%v", next, err)
	}
	if _, err := s.Change(w.ID, w.Site, w.Revision, task.ID, 1, "done"); !errors.Is(err, ErrConflict) {
		t.Fatal("stale task revision accepted")
	}
	next, err = s.Change(w.ID, w.Site, w.Revision, task.ID, 2, "done")
	if err != nil || next.Summary.Open != 0 || next.Summary.Done != 1 || *next.Summary.CompletionPercent != 100 || len(next.Activity) != 3 {
		t.Fatalf("done=%+v err=%v", next, err)
	}
	next, err = s.Change(w.ID, w.Site, w.Revision, task.ID, 3, "done")
	if err != nil || len(next.Activity) != 3 || next.Tasks[0].Revision != 3 {
		t.Fatal("no-op created event/revision")
	}
	reopened, err := Open(path, "org-one")
	if err != nil {
		t.Fatal(err)
	}
	got, err := New(reopened, registry, source, nil).List(w.ID, w.Site)
	if err != nil || got.Tasks[0].Status != "done" || len(got.Activity) != 3 {
		t.Fatalf("reopen=%+v err=%v", got, err)
	}
	got.Tasks[0].Evidence.Key = "mutated"
	again, _ := s.List(w.ID, w.Site)
	if again.Tasks[0].Evidence.Key != "webdesign" {
		t.Fatal("snapshot mutated state")
	}
	dismissed, err := s.Change(w.ID, w.Site, w.Revision, task.ID, 3, "dismissed")
	if err != nil || dismissed.Summary.Dismissed != 1 || dismissed.Summary.Total != 1 || dismissed.Summary.CompletionPercent != nil {
		t.Fatalf("dismissed=%+v err=%v", dismissed, err)
	}
}

func TestQueueRejectsForeignStaleAndUnverifiedFindings(t *testing.T) {
	s, registry, w, source, _ := testService(t)
	if _, _, err := s.Save(t.Context(), w.ID, "https://other.no/", w.Revision, "Q4", "webdesign"); !errors.Is(err, ErrConflict) {
		t.Fatalf("wrong origin: %v", err)
	}
	if _, _, err := s.Save(t.Context(), w.ID, w.Site, w.Revision+1, "Q4", "webdesign"); !errors.Is(err, ErrConflict) {
		t.Fatal("stale website accepted")
	}
	if source.calls != 0 {
		t.Fatal("invalid context reached source")
	}
	for _, query := range []string{"missing", "", "bad\nkey"} {
		if _, _, err := s.Save(t.Context(), w.ID, w.Site, w.Revision, "Q4", query); err == nil {
			t.Fatal("invalid finding accepted")
		}
	}
	if _, _, err := s.Save(t.Context(), w.ID, w.Site, w.Revision, "Q9", "webdesign"); !errors.Is(err, ErrInvalid) {
		t.Fatal("invalid quarter")
	}
	source.projection.Status = pulse.StatusUnavailable
	if _, _, err := s.Save(t.Context(), w.ID, w.Site, w.Revision, "Q4", "webdesign"); !errors.Is(err, ErrSourceUnavailable) {
		t.Fatal("unavailable source fabricated task")
	}
	source.projection.Status = pulse.StatusConnected
	source.projection.Site = "https://other.no/"
	if _, _, err := s.Save(t.Context(), w.ID, w.Site, w.Revision, "Q4", "webdesign"); !errors.Is(err, ErrSourceUnavailable) {
		t.Fatal("foreign source accepted")
	}
	source.projection.Site = w.Site
	old := saveFinding(t, s, w)
	if _, err := s.Change(w.ID, w.Site, w.Revision, old.Tasks[0].ID, 1, "invalid"); !errors.Is(err, ErrInvalid) {
		t.Fatal("invalid status")
	}
	if _, err := s.Change(w.ID, w.Site, w.Revision, "task_00000000000000000000000000000000", 1, "done"); !errors.Is(err, ErrNotFound) {
		t.Fatal("missing task")
	}
	changed, err := registry.Change(t.Context(), w.ID, "https://agenci.no/", w.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.List(w.ID, w.Site); !errors.Is(err, ErrConflict) {
		t.Fatal("old origin readable")
	}
	list, err := s.List(changed.ID, changed.Site)
	if err != nil || len(list.Tasks) != 0 || len(list.Activity) != 0 || list.Summary.CompletionPercent != nil {
		t.Fatalf("cross site leak: %+v %v", list, err)
	}
	if _, err := s.Change(changed.ID, changed.Site, changed.Revision, old.Tasks[0].ID, 1, "done"); !errors.Is(err, ErrNotFound) {
		t.Fatal("old site task editable")
	}
}

func TestChangedWebsiteDuringSourceReadCannotSaveOldEvidence(t *testing.T) {
	s, registry, w, source, _ := testService(t)
	source.during = func() {
		_, err := registry.Change(t.Context(), w.ID, "https://agenci.no/", w.Revision)
		if err != nil {
			t.Error(err)
		}
	}
	if _, _, err := s.Save(t.Context(), w.ID, w.Site, w.Revision, "Q4", "webdesign"); !errors.Is(err, ErrConflict) {
		t.Fatalf("changed context: %v", err)
	}
}

func TestConcurrentDuplicateSavesProduceOneTask(t *testing.T) {
	s, _, w, source, _ := testService(t)
	// Repository serialization is tested independently of the deliberately simple source stub.
	evidence := Evidence{Kind: "gsc_opportunity", Key: "webdesign", OpportunityType: "title_meta", Period: source.projection.Period, Clicks: 2, Impressions: 200, CTR: 0.01, Position: 7, ObservedAt: source.projection.UpdatedAt}
	var group sync.WaitGroup
	for range 10 {
		group.Go(func() {
			if _, err := s.repo.create(w.ID, w.Site, evidence, "2026-10-22T12:00:00Z"); err != nil {
				t.Error(err)
			}
		})
	}
	group.Wait()
	snapshot, _ := s.List(w.ID, w.Site)
	if len(snapshot.Tasks) != 1 || len(snapshot.Activity) != 1 {
		t.Fatalf("duplicate=%+v", snapshot)
	}
}

func TestSaveFullYearOpportunityUsesBoundedYearEvidence(t *testing.T) {
	s, _, w, source, _ := testService(t)
	source.projection.Period = pulse.Period{StartDate: "2026-01-01", EndDate: "2026-10-21"}
	got, created, err := s.Save(t.Context(), w.ID, w.Site, w.Revision, "FY", "webdesign")
	if err != nil || !created || got.Tasks[0].Evidence.Period.StartDate != "2026-01-01" {
		t.Fatal("full-year opportunity rejected", err)
	}
}
