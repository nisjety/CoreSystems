package websites

import (
	"context"
	"strings"
	"testing"
)

func TestDNSWorkflowIsRetiredWithoutChangingRecords(t *testing.T) {
	s := newTestService(t, nil)
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Challenge(w.ID); err == nil {
		t.Fatal("retired DNS challenge generated")
	}
	if _, err = s.VerifyDNS(t.Context(), w.ID); err == nil {
		t.Fatal("retired DNS verification ran")
	}
	r, _ := s.Resolve(w.ID, w.Site)
	if r.Website.Revision != w.Revision || r.DNS != nil {
		t.Fatal("retired workflow changed registry")
	}
}

func TestRetiredDNSWorkflowPreservesHistoricalRegistryRecords(t *testing.T) {
	s := newTestService(t, nil)
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	historical := DNSChallenge{Name: "_studio-verification.example.com", Value: "studio-verification=" + strings.Repeat("a", 43), ExpiresAt: "2026-10-20T12:00:00Z"}
	if err = s.repo.Update(func(records []Record) ([]Record, error) {
		next := copyRecords(records)
		next[0].Website.Ownership = Ownership{Status: "dns_verified", CheckedAt: "2026-10-20T11:00:00Z"}
		next[0].DNS = &historical
		return next, nil
	}); err != nil {
		t.Fatal(err)
	}
	restored, err := Open(s.repo.path, s.OrganizationID())
	if err != nil {
		t.Fatal(err)
	}
	service := NewService(restored, nil, s.now)
	if _, err = service.Challenge(w.ID); err == nil {
		t.Fatal("historical record reactivated DNS flow")
	}
	if _, err = service.VerifyDNS(t.Context(), w.ID); err == nil {
		t.Fatal("historical record reactivated DNS lookup")
	}
	got, err := service.Resolve(w.ID, w.Site)
	if err != nil || got.Website.Ownership.Status != "dns_verified" || got.DNS == nil || *got.DNS != historical {
		t.Fatal(got, err)
	}
}

func TestGA4ReportingAliasUsesCurrentProviderResourceNotSearchOwnership(t *testing.T) {
	s := newTestService(t, func(context.Context, string) Evidence {
		return Evidence{SearchConsole: Access{Status: "not_found"}, GA4: Access{Status: "connected", ConnectionID: "ga4-one", PropertyID: "123", StreamID: "456", MeasurementID: "G-ABCDE12345"}}
	})
	w, _, err := s.Add(t.Context(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.MapGA4ReportingSite(t.Context(), w.ID, "https://www.example.com/", w.Revision); err != nil {
		t.Fatal("provider-authorized exact www alias blocked", err)
	}
}
