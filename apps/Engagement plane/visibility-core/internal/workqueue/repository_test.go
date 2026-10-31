package workqueue

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func TestStorageRejectsForeignCorruptOrUnprotectedState(t *testing.T) {
	s, _, w, _, path := testService(t)
	saveFinding(t, s, w)
	if _, err := Open(path, "other-org"); err == nil {
		t.Fatal("foreign organization accepted")
	}
	if _, err := Open("relative.json", "org-one"); err == nil {
		t.Fatal("relative file accepted")
	}
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(path, "org-one"); err == nil {
		t.Fatal("public file accepted")
	}
	if err := os.Chmod(path, 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(filepath.Dir(path), "link.json")
	if err := os.Symlink(path, link); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(link, "org-one"); err == nil {
		t.Fatal("symlink accepted")
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, value := range []string{string(raw) + "{}", strings.Replace(string(raw), `"kind":"gsc_opportunity"`, `"kind":"ads_write"`, 1), strings.Replace(string(raw), `"clicks":2`, `"clicks":2000`, 1), `{"version":1,"organizationId":"org-one","tasks":null,"activity":[]}`, strings.Repeat("x", maxFileBytes+1)} {
		if err := os.WriteFile(path, []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := Open(path, "org-one"); err == nil {
			t.Fatal("invalid saved state accepted")
		}
	}
	if err := os.Chmod(filepath.Dir(path), 0755); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(path, "org-one"); err == nil {
		t.Fatal("public directory accepted")
	}
}

func TestEvidenceValidationAndBoundedHistory(t *testing.T) {
	s, _, w, source, _ := testService(t)
	for _, candidate := range []pulse.Opportunity{
		{SearchRow: pulse.SearchRow{Key: "webdesign", Clicks: 2, Impressions: 200, CTR: 0.01, Position: 7}, Type: "other"},
		{SearchRow: pulse.SearchRow{Key: "webdesign", Clicks: 2, Impressions: 200, CTR: 0.8, Position: 7}, Type: "title_meta"},
	} {
		source.projection.Opportunities = []pulse.Opportunity{candidate}
		if _, _, err := s.Save(context.Background(), w.ID, w.Site, w.Revision, "Q4", "webdesign"); !errors.Is(err, ErrSourceUnavailable) {
			t.Fatalf("malformed evidence=%v", err)
		}
	}
	source.projection.Opportunities = []pulse.Opportunity{{SearchRow: pulse.SearchRow{Key: "webdesign", Clicks: 2, Impressions: 200, CTR: 0.01, Position: 7}, Type: "title_meta"}}
	first := saveFinding(t, s, w)
	template := first.Tasks[0]
	next := cloneState(s.repo.state)
	for i := 1; i < maxTasks; i++ {
		task := template
		task.ID = newID("task_")
		task.Evidence.Key = task.ID
		next.Tasks = append(next.Tasks, task)
	}
	if err := s.repo.commit(next); err != nil {
		t.Fatal(err)
	}
	evidence := template.Evidence
	evidence.Key = "new finding"
	if _, err := s.repo.create(w.ID, w.Site, evidence, template.CreatedAt); !errors.Is(err, ErrCapacity) {
		t.Fatal("capacity not enforced")
	}
	events := []Activity{}
	for range maxActivity + 10 {
		events = appendActivity(events, Activity{ID: newID("evt_"), TaskID: template.ID, Kind: "status_changed", Status: "done", OccurredAt: template.CreatedAt})
	}
	if len(events) != maxActivity {
		t.Fatal("unbounded activity")
	}
	broken := cloneState(next)
	broken.Activity = append(broken.Activity, Activity{ID: newID("evt_"), TaskID: "task_missing", Kind: "created", Status: "todo", OccurredAt: template.CreatedAt})
	if validateState(broken) == nil {
		t.Fatal("orphaned activity accepted")
	}
	var public map[string]any
	encoded, _ := json.Marshal(s.repo.snapshot(w.ID, w.Site))
	_ = json.Unmarshal(encoded, &public)
	if _, ok := public["organizationId"]; ok {
		t.Fatal("private disk metadata exposed")
	}
}
