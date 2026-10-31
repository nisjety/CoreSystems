package websites

import (
	"context"
	"path/filepath"
	"testing"
	"time"
)

func newTestService(t *testing.T, discovery Discoverer) *Service {
	t.Helper()
	repo, err := Open(filepath.Join(privateDir(t), "websites.json"), "org-one")
	if err != nil {
		t.Fatal(err)
	}
	return NewService(repo, discovery, func() time.Time { return time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC) })
}

func TestAddDiscoverAndURLChangeInvalidatesEveryBinding(t *testing.T) {
	service := newTestService(t, func(_ context.Context, site string) Evidence {
		if site != "https://example.com/" {
			return Evidence{SearchConsole: Access{Status: "not_found"}, GA4: Access{Status: "requires_consent"}}
		}
		return Evidence{SearchConsole: Access{Status: "connected", ConnectionID: "conn-a", Property: "sc-domain:example.com", AccessLevel: "siteOwner"}, GA4: Access{Status: "connected", ConnectionID: "conn-b", PropertyID: "123", StreamID: "456", MeasurementID: "G-ABCDE12345"}}
	})
	created, _, err := service.Add(context.Background(), "example.com")
	if err != nil {
		t.Fatal(err)
	}
	if created.Integrations.SearchConsole.Status != "connected" || created.Ownership.Status != "google_owner" || created.Integrations.GA4.PropertyID != "123" {
		t.Fatalf("missing verified mapping: %+v", created)
	}
	if _, _, err := service.Add(context.Background(), "https://example.com/"); err != nil {
		t.Fatal(err)
	}
	if len(service.List()) != 1 {
		t.Fatal("duplicate site")
	}
	updated, err := service.Change(context.Background(), created.ID, "https://other.example/", created.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if updated.Ownership.Status != "unverified" || updated.Integrations.GA4.PropertyID != "" || updated.Integrations.SearchConsole.Property != "" || updated.Integrations.Clarity.ProjectID != "" {
		t.Fatal("old mappings leaked")
	}
	if _, err := service.Resolve(created.ID, "https://example.com/"); err == nil {
		t.Fatal("old URL still resolved")
	}
	if _, err := service.Change(context.Background(), created.ID, "https://third.example/", created.Revision); err == nil {
		t.Fatal("stale change accepted")
	}
}

func TestReadAccessIsNotOwnershipAndUnverifiedMappingsAreNotConnected(t *testing.T) {
	service := newTestService(t, func(context.Context, string) Evidence {
		return Evidence{SearchConsole: Access{Status: "connected", ConnectionID: "conn-a", Property: "sc-domain:example.com", AccessLevel: "siteFullUser"}, GA4: Access{Status: "requires_consent"}}
	})
	site, _, err := service.Add(context.Background(), "https://example.com/")
	if err != nil {
		t.Fatal(err)
	}
	if site.Ownership.Status != "unverified" || site.Integrations.SearchConsole.Status != "connected" {
		t.Fatal("read access confused with ownership")
	}
	site, err = service.MapClarity(site.ID, "abc123", site.Revision)
	if err != nil || site.Integrations.Clarity.Status != "pending_verification" {
		t.Fatal("Clarity mapping presented as authorized")
	}
	if _, err := service.MapClarity(site.ID, "https://evil.example/", site.Revision); err == nil {
		t.Fatal("arbitrary Clarity URL accepted")
	}
}

func TestGA4ReportingSiteUsesCurrentResourceEvenWithoutGoogleOwnership(t *testing.T) {
	owner := true
	service := newTestService(t, func(_ context.Context, site string) Evidence {
		if site != "https://triodelab.no/" {
			return Evidence{SearchConsole: Access{Status: "not_found"}, GA4: Access{Status: "not_found"}}
		}
		gsc := Access{Status: "connected", ConnectionID: "gsc-one", Property: "sc-domain:triodelab.no", AccessLevel: "siteOwner"}
		if !owner {
			gsc.AccessLevel = "siteFullUser"
		}
		return Evidence{SearchConsole: gsc, GA4: Access{Status: "connected", ConnectionID: "ga4-one", PropertyID: "560102923", StreamID: "16177443323", MeasurementID: "G-ZEWPHF6X3K"}}
	})
	website, _, err := service.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	if website.Ownership.Status != "google_owner" {
		t.Fatal("missing owner proof")
	}
	for _, bad := range []string{"https://other.no/", "https://blog.triodelab.no/", "https://www.triodelab.no/path", "https://www.triodelab.no:443/", "https://user@www.triodelab.no/"} {
		if _, err := service.MapGA4ReportingSite(t.Context(), website.ID, bad, website.Revision); err != ErrInvalid {
			t.Errorf("accepted %q: %v", bad, err)
		}
	}
	updated, err := service.MapGA4ReportingSite(t.Context(), website.ID, "https://www.triodelab.no/", website.Revision)
	if err != nil {
		t.Fatal(err)
	}
	if updated.Integrations.GA4.ReportingSite != "https://www.triodelab.no/" || updated.Integrations.SearchConsole.Property != "sc-domain:triodelab.no" || updated.Integrations.GA4.StreamID != "16177443323" {
		t.Fatalf("changed bindings: %+v", updated)
	}
	reopened, err := Open(service.repo.path, "org-one")
	if err != nil || reopened.List()[0].Website.Integrations.GA4.ReportingSite != "https://www.triodelab.no/" {
		t.Fatalf("mapping not durably persisted: %v", err)
	}
	if _, err := service.MapGA4ReportingSite(t.Context(), website.ID, website.Site, website.Revision); err != ErrConflict {
		t.Fatalf("stale revision: %v", err)
	}
	updated, err = service.Verify(t.Context(), website.ID)
	if err != nil || updated.Integrations.GA4.ReportingSite != "https://www.triodelab.no/" {
		t.Fatalf("owner reverify: %+v %v", updated, err)
	}
	owner = false
	updated, err = service.Verify(t.Context(), website.ID)
	if err != nil || updated.Integrations.GA4.ReportingSite != "https://www.triodelab.no/" || updated.Integrations.SearchConsole.AccessLevel != "siteFullUser" {
		t.Fatalf("owner lost: %+v %v", updated, err)
	}
	updated, err = service.MapGA4ReportingSite(t.Context(), website.ID, "https://www.triodelab.no/", updated.Revision)
	if err != nil {
		t.Fatal("provider-authorized reader blocked", err)
	}
	changed, err := service.Change(t.Context(), website.ID, "https://other.example/", updated.Revision)
	if err != nil || changed.Integrations.GA4.ReportingSite != "" || changed.Ownership.Status != "unverified" {
		t.Fatalf("website URL retained alternate host: %+v %v", changed, err)
	}
}

func TestGA4ReportingSitePatchChecksFreshResourceWithoutRevisionBump(t *testing.T) {
	owner := true
	checks := 0
	service := newTestService(t, func(_ context.Context, site string) Evidence {
		checks++
		level := "siteOwner"
		if !owner {
			level = "siteFullUser"
		}
		ga4 := Access{Status: "connected", ConnectionID: "ga4-one", PropertyID: "560102923", StreamID: "16177443323", MeasurementID: "G-ZEWPHF6X3K"}
		if !owner {
			ga4 = Access{Status: "not_found"}
		}
		return Evidence{SearchConsole: Access{Status: "connected", ConnectionID: "gsc-one", Property: "sc-domain:triodelab.no", AccessLevel: level}, GA4: ga4}
	})
	website, _, err := service.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	baselineChecks := checks
	owner = false // Stored owner evidence is now stale.
	if _, err := service.MapGA4ReportingSite(t.Context(), website.ID, "https://www.triodelab.no/", website.Revision); err != ErrInvalid {
		t.Fatalf("stale provider resource accepted: %v", err)
	}
	stored, err := service.Resolve(website.ID, "")
	if err != nil || stored.Website.Revision != website.Revision || stored.Website.Integrations.GA4.ReportingSite != "" || checks != baselineChecks+1 {
		t.Fatalf("rejected mapping changed record or did not refresh proof: %+v checks=%d err=%v", stored.Website, checks, err)
	}
	owner = true
	updated, err := service.MapGA4ReportingSite(t.Context(), website.ID, "https://www.triodelab.no/", website.Revision)
	if err != nil || updated.Revision != website.Revision+1 || updated.Integrations.GA4.ReportingSite == "" {
		t.Fatalf("fresh proof mapping: %+v %v", updated, err)
	}
}

func TestGA4ReportingSiteDoesNotRequireDomainOwnership(t *testing.T) {
	property := "https://triodelab.no/"
	service := newTestService(t, func(_ context.Context, site string) Evidence {
		return Evidence{SearchConsole: Access{Status: "connected", ConnectionID: "gsc-one", Property: property, AccessLevel: "siteOwner"}, GA4: Access{Status: "connected", ConnectionID: "ga4-one", PropertyID: "560102923", StreamID: "16177443323", MeasurementID: "G-ZEWPHF6X3K"}}
	})
	website, _, err := service.Add(t.Context(), "https://triodelab.no/")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := service.MapGA4ReportingSite(t.Context(), website.ID, "https://www.triodelab.no/", website.Revision); err != nil {
		t.Fatalf("provider-authorized prefix context blocked: %v", err)
	}
	property = "sc-domain:triodelab.no"
	updated, err := service.Verify(t.Context(), website.ID)
	if err != nil {
		t.Fatal(err)
	}
	updated, err = service.MapGA4ReportingSite(t.Context(), website.ID, "https://www.triodelab.no/", updated.Revision)
	if err != nil {
		t.Fatal(err)
	}
	property = "https://triodelab.no/"
	updated, err = service.Verify(t.Context(), website.ID)
	if err != nil || updated.Integrations.GA4.ReportingSite != "https://www.triodelab.no/" {
		t.Fatalf("prefix-only reverify retained alternate: %+v %v", updated, err)
	}
}
