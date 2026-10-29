package analyticssync

import "testing"

func TestConnectionReconciliationPreservesPauseAndRepairsMissingJob(t *testing.T) {
	s, r, _, _, _ := fixture(t)
	w := r.record.Website
	if err := s.EnsureConnectionSync(w.ID, w.Site, w.Revision, "Q4"); err != nil {
		t.Fatal(err)
	}
	first, _ := s.Get(w.ID, w.Site, "Q4")
	if err := s.EnsureConnectionSync(w.ID, w.Site, w.Revision, "Q4"); err != nil {
		t.Fatal(err)
	}
	again, _ := s.Get(w.ID, w.Site, "Q4")
	if first.JobID != again.JobID {
		t.Fatal("duplicate initial collection")
	}
	if _, err := s.SetEnabled(w.ID, w.Site, w.Revision, "Q4", false); err != nil {
		t.Fatal(err)
	}
	r.record.Website.Revision++
	if err := s.EnsureConnectionSync(w.ID, w.Site, r.record.Website.Revision, "Q4"); err != nil {
		t.Fatal(err)
	}
	if len(s.repo.read().Entries) != 1 || s.repo.read().Entries[0].Enabled {
		t.Fatal("connection update resumed an explicit pause")
	}
}
