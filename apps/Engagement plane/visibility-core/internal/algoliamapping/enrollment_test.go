package algoliamapping

import "testing"

func TestEnrollmentsAreLocalCurrentMappingIntentionsOnly(t *testing.T) {
	s, r, c, _ := fixture(t)
	w := r.record.Website
	if got := s.Enrollments(); len(got) != 0 {
		t.Fatal(got)
	}
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "site"); err != nil {
		t.Fatal(err)
	}
	calls := c.calls
	r.record.Website.Ownership.Status = "unverified"
	got := s.Enrollments()
	if len(got) != 1 || got[0].WebsiteID != w.ID || got[0].WebsiteRevision != w.Revision || got[0].Fingerprint == "" || c.calls != calls {
		t.Fatal(got, c.calls, calls)
	}
	r.record.Website.Revision++
	if got = s.Enrollments(); len(got) != 0 {
		t.Fatal("stale revision enrolled", got)
	}
	r.record.Website.Revision = w.Revision
	if _, err := s.Delete(w.ID, w.Site, w.Revision); err != nil {
		t.Fatal(err)
	}
	if got = s.Enrollments(); len(got) != 0 {
		t.Fatal("tombstone enrolled", got)
	}
}
