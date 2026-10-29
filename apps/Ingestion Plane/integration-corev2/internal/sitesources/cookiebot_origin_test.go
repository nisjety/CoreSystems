package sitesources

import (
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestCookiebotFindsAuthorizedDomainAliasAfterTwoMissingResources(t *testing.T) {
	for _, status := range []int{404, 403, 429} {
		calls := 0
		client := &http.Client{Transport: roundTrip(func(r *http.Request) (*http.Response, error) {
			calls++
			if strings.Contains(r.URL.Path, "/domain/triodelab.no/") {
				v := response(`{}`)
				v.StatusCode = status
				return v, nil
			}
			if strings.Contains(r.URL.Path, "/consent/stats") {
				return response(consentJSON), nil
			}
			return response(inventoryJSON), nil
		})}
		service, err := NewService("", client, func() time.Time { return current })
		if err != nil {
			t.Fatal(err)
		}
		got := service.cookiebot(t.Context(), cookieBinding{DomainGroupID: group, Domain: "triodelab.no", Culture: "en"}, "synthetic", period(current), func() bool { return true })
		if status == 404 {
			if calls != 4 || got.ReportingDomain != "www.triodelab.no" || got.Consents.Status != "connected" {
				t.Fatal(got, calls)
			}
		} else if calls != 2 || got.ReportingDomain != "triodelab.no" {
			t.Fatal("access or quota failure triggered discovery")
		}
	}
}
