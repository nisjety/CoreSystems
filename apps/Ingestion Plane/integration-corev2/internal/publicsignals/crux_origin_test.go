package publicsignals

import (
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"testing"
)

func TestCrUXDiscoversExactWWWCounterpartOnlyWhenCoverageIsMissing(t *testing.T) {
	for _, site := range []string{"https://triodelab.no/", "https://www.triodelab.no/"} {
		for _, factor := range []string{"PHONE", "DESKTOP"} {
			var origins []string
			alternate := strings.TrimSuffix(cruxAlternateSite(site), "/")
			s := fixtureService(func(r *http.Request) (*http.Response, error) {
				if r.URL.Host != "chromeuxreport.googleapis.com" || r.URL.Path != "/v1/records:queryRecord" {
					t.Fatal("origin discovery changed egress")
				}
				var body struct{ Origin, FormFactor string }
				_ = json.NewDecoder(r.Body).Decode(&body)
				origins = append(origins, body.Origin)
				if body.FormFactor != factor {
					t.Fatal("device changed during discovery")
				}
				if len(origins) == 1 {
					return response(404, `{"error":{"status":"NOT_FOUND"}}`), nil
				}
				if body.Origin != alternate {
					t.Fatalf("unexpected candidate %s", body.Origin)
				}
				return response(200, strings.ReplaceAll(cruxFixture(factor), "https://triodelab.no", alternate)), nil
			})
			got := s.crux(context.Background(), site, factor)
			if len(origins) != 2 || origins[0] != strings.TrimSuffix(site, "/") || got.ReportingOrigin != alternate || got.Status != "connected" || got.Metrics == nil {
				t.Fatalf("no discovered report: %+v %v", got, origins)
			}
		}
	}
}

func TestCrUXPrefersSelectedOriginAndDoesNotProbeOnProviderFailures(t *testing.T) {
	for _, status := range []int{200, 400, 403, 429, 500, 503, 504} {
		calls := 0
		s := fixtureService(func(*http.Request) (*http.Response, error) {
			calls++
			return response(status, cruxFixture("PHONE")), nil
		})
		got := s.crux(context.Background(), "https://triodelab.no/", "PHONE")
		if calls != 1 {
			t.Fatalf("status %d triggered extra discovery", status)
		}
		if status == 200 && (got.ReportingOrigin != "https://triodelab.no" || got.Status != "connected") {
			t.Fatal("selected origin was not retained")
		}
	}
	for _, status := range []int{404, 429, 500} {
		calls := 0
		s := fixtureService(func(*http.Request) (*http.Response, error) {
			calls++
			if calls == 1 {
				return response(404, ""), nil
			}
			return response(status, ""), nil
		})
		got := s.crux(context.Background(), "https://triodelab.no/", "PHONE")
		if calls != 2 || got.Reason != statusReason(status) || got.ReportingOrigin != "" || got.Metrics != nil {
			t.Fatalf("fallback failure was hidden: %+v", got)
		}
	}
}

func TestCrUXDiscoveryRejectsUnrelatedReturnedOrigins(t *testing.T) {
	for _, origin := range []string{"https://other.no", "https://blog.triodelab.no", "https://www.triodelab.no:443", "https://www.triodelab.no/"} {
		calls := 0
		s := fixtureService(func(*http.Request) (*http.Response, error) {
			calls++
			if calls == 1 {
				return response(404, ""), nil
			}
			return response(200, strings.ReplaceAll(cruxFixture("PHONE"), "https://triodelab.no", origin)), nil
		})
		got := s.crux(context.Background(), "https://triodelab.no/", "PHONE")
		if got.Reason != "source_unavailable" || got.ReportingOrigin != "" {
			t.Fatalf("unrequested provider origin accepted: %s", origin)
		}
	}
	if cruxAlternateSite("https://www.com/") != "" {
		t.Fatal("single-label parent became a candidate")
	}
}
