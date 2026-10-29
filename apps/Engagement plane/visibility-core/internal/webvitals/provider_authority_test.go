package webvitals

import "testing"

func TestAuthorizedInstallationDoesNotRequireOurDNSProof(t *testing.T) {
	s, r, _, _ := fixture(t)
	r.record.Website.Ownership.Status = "unverified"
	w := r.record.Website
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "production"); err != nil {
		t.Fatal("authorized installation incorrectly requires DNS proof", err)
	}
}
