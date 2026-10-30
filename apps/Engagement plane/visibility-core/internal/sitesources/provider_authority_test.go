package sitesources

import "testing"

func TestProviderAuthorizedReportsDoNotRequireOurDNSProof(t *testing.T) {
	s, r, _, _ := fixture(t)
	r.record.Website.Ownership.Status = "unverified"
	w := r.record.Website
	if _, err := s.Sync(t.Context(), w.ID, w.Site, w.Revision, "algolia"); err != nil {
		t.Fatal("provider-authorized report incorrectly requires DNS proof", err)
	}
}
