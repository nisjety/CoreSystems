package algoliamapping

import "testing"

func TestProviderAuthorizedMappingDoesNotRequireOurDNSProof(t *testing.T) {
	s, r, _, _ := fixture(t)
	r.record.Website.Ownership.Status = "unverified"
	w := r.record.Website
	if _, err := s.Put(t.Context(), w.ID, w.Site, w.Revision, "conn_1234", "APP123", "public_index"); err != nil {
		t.Fatal("provider-authorized mapping incorrectly requires DNS proof", err)
	}
	if a, err := s.Authorize(t.Context(), r.record); err != nil || !a.Active {
		t.Fatal("active provider authority lost", a, err)
	}
}
