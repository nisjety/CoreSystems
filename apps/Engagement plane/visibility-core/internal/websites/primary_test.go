package websites

import (
	"context"
	"errors"
	"testing"
)

func noDiscovery(context.Context, string) Evidence { return Evidence{} }

// orgCoreHolds is a primary source that answers with site, as org-core would.
func orgCoreHolds(site string) PrimarySource {
	return func(context.Context) (string, error) { return site, nil }
}

func canonical(t *testing.T, site string) string {
	t.Helper()
	value, err := canonicalInput(site)
	if err != nil {
		t.Fatalf("canonicalInput(%q): %v", site, err)
	}
	return value
}

func primaries(sites []Website) (count int, site string) {
	for _, s := range sites {
		if s.Primary {
			count++
			site = s.Site
		}
	}
	return count, site
}

func TestThePrimarySiteIsTheOneOrgCoreHoldsAndIsRegisteredOnce(t *testing.T) {
	t.Parallel()

	service := newTestService(t, noDiscovery)
	service.SetPrimarySource(orgCoreHolds("https://triodelab.no/"))
	want := canonical(t, "https://triodelab.no/")

	sites, status := service.ListWithPrimary(context.Background())
	if status != PrimaryFromOrgCore || len(sites) != 1 || !sites[0].Primary || sites[0].Site != want {
		t.Fatalf("first list = %+v, status %q", sites, status)
	}
	sites, _ = service.ListWithPrimary(context.Background())
	if len(sites) != 1 {
		t.Errorf("a second list registered the primary again: %d sites", len(sites))
	}
	for _, stored := range service.List() {
		if stored.Primary {
			t.Error("the primary mark was stored; it must be set on read only")
		}
	}
}

func TestOtherSitesStayUnmarkedAndCanStillBeAdded(t *testing.T) {
	t.Parallel()

	service := newTestService(t, noDiscovery)
	if _, _, err := service.Add(context.Background(), "https://other-firma.no/"); err != nil {
		t.Fatal(err)
	}
	service.SetPrimarySource(orgCoreHolds("https://triodelab.no/"))

	sites, _ := service.ListWithPrimary(context.Background())
	if len(sites) != 2 {
		t.Fatalf("sites = %d, want the primary and the other site", len(sites))
	}
	if count, site := primaries(sites); count != 1 || site != canonical(t, "https://triodelab.no/") {
		t.Errorf("primaries = %d at %q, want exactly the org-core site", count, site)
	}
	if _, _, err := service.Add(context.Background(), "https://third-firma.no/"); err != nil {
		t.Fatalf("adding another site after the primary: %v", err)
	}
	sites, _ = service.ListWithPrimary(context.Background())
	if len(sites) != 3 {
		t.Errorf("sites = %d after adding a third, want 3", len(sites))
	}
	if count, _ := primaries(sites); count != 1 {
		t.Errorf("primaries = %d after adding a site, want 1", count)
	}
}

func TestWhenOrgCoreChangesTheWebsiteTheMarkMoves(t *testing.T) {
	t.Parallel()

	service := newTestService(t, noDiscovery)
	current := "https://old-firma.no/"
	service.SetPrimarySource(func(context.Context) (string, error) { return current, nil })
	service.ListWithPrimary(context.Background())

	current = "https://new-firma.no/"
	sites, _ := service.ListWithPrimary(context.Background())
	if count, site := primaries(sites); count != 1 || site != canonical(t, "https://new-firma.no/") {
		t.Errorf("after the change: %d primaries at %q", count, site)
	}
	if len(sites) != 2 {
		t.Errorf("the old site was dropped: %d sites", len(sites))
	}
}

func TestWhenOrgCoreCannotBeReadEverySiteIsStillListed(t *testing.T) {
	t.Parallel()

	service := newTestService(t, noDiscovery)
	if _, _, err := service.Add(context.Background(), "https://triodelab.no/"); err != nil {
		t.Fatal(err)
	}
	service.SetPrimarySource(func(context.Context) (string, error) { return "", errors.New("org-core down") })

	sites, status := service.ListWithPrimary(context.Background())
	if status != PrimaryUnavailable || len(sites) != 1 {
		t.Errorf("status %q with %d sites, want unavailable and the registered site", status, len(sites))
	}
	if count, _ := primaries(sites); count != 0 {
		t.Errorf("a site was marked primary without org-core's answer")
	}
}

func TestWithoutAPrimarySourceTheRegistryListsAsBefore(t *testing.T) {
	t.Parallel()

	service := newTestService(t, noDiscovery)
	if _, _, err := service.Add(context.Background(), "https://triodelab.no/"); err != nil {
		t.Fatal(err)
	}
	sites, status := service.ListWithPrimary(context.Background())
	if status != PrimaryNotConfigured || len(sites) != 1 {
		t.Errorf("status %q with %d sites, want not_configured and the registered site", status, len(sites))
	}
	if count, _ := primaries(sites); count != 0 {
		t.Error("a site was marked primary without an org-core source")
	}
}
