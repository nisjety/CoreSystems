package sitesources

import (
	"net/http"
	"testing"
)

func TestInstallationMetadataIsFreshScopedAndProviderFree(t *testing.T) {
	service, err := NewService(setupManifest(t, nil), &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
		t.Fatal("metadata fetched provider data")
		return nil, nil
	})}, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, provider := range []string{"cookiebot", "algolia"} {
		in := input(provider)
		if ok, err := service.Configured("org-1", in); err != nil || !ok {
			t.Fatal("missing binding", ok, err)
		}
		if ok, _ := service.Configured("org-2", in); ok {
			t.Fatal("cross-org binding")
		}
		in.WebsiteRevision++
		if ok, _ := service.Configured("org-1", in); ok {
			t.Fatal("stale binding")
		}
		in = input(provider)
		in.Site = "https://www.triodelab.no/"
		if ok, _ := service.Configured("org-1", in); ok {
			t.Fatal("different installation accepted")
		}
	}
}
