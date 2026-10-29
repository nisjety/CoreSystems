package publicsignals

import (
	"context"
	"net/http"
	"strings"
	"testing"
)

func TestArchiveDiscoversHostAndKeepsProvenance(t *testing.T) {
	for _, status := range []int{404, 500, 504} {
		for _, site := range []string{"https://triodelab.no/", "https://www.triodelab.no/"} {
			alternate := strings.TrimSuffix(strings.TrimPrefix(cruxAlternateSite(site), "https://"), "/")
			s := fixtureService(func(r *http.Request) (*http.Response, error) {
				if r.URL.Path == "/collinfo.json" {
					return response(200, indexes), nil
				}
				host := r.URL.Query().Get("url")
				if host != alternate {
					return response(status, ""), nil
				}
				return response(200, strings.ReplaceAll(captureFixture, "triodelab.no", alternate)), nil
			})
			got := s.archive(context.Background(), site)
			if got.Status != "connected" || got.ReportingHost != alternate || len(got.Captures) == 0 || !strings.Contains(got.Captures[0].URL, alternate) {
				t.Fatalf("missing alias evidence: %+v", got)
			}
		}
	}
}
func TestArchiveHostDiscoveryNeverBypassesThrottleOrInvalidEvidence(t *testing.T) {
	for _, status := range []int{200, 401, 403, 429, 503} {
		calls := 0
		s := fixtureService(func(r *http.Request) (*http.Response, error) {
			if r.URL.Path == "/collinfo.json" {
				return response(200, indexes), nil
			}
			calls++
			return response(status, "not json"), nil
		})
		got := s.archive(context.Background(), "https://triodelab.no/")
		if calls != 1 || got.ReportingHost != "" || got.Status != "unavailable" {
			t.Fatal("unsafe extra discovery", got, calls)
		}
	}
}

func TestArchiveCanonicalIndexReturnsCounterpartCaptures(t *testing.T) {
	for _, site := range []string{"https://triodelab.no/", "https://www.triodelab.no/"} {
		requested := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
		alternate := strings.TrimSuffix(strings.TrimPrefix(cruxAlternateSite(site), "https://"), "/")
		for _, mixed := range []bool{false, true} {
			calls := 0
			s := fixtureService(func(r *http.Request) (*http.Response, error) {
				if r.URL.Path == "/collinfo.json" {
					return response(200, indexes), nil
				}
				calls++
				if r.URL.Query().Get("url") != requested {
					t.Fatal("canonical response should not need another query")
				}
				body := strings.ReplaceAll(captureFixture, "triodelab.no", alternate)
				if mixed {
					body += "\n" + strings.ReplaceAll(captureFixture, "triodelab.no", requested)
				}
				return response(200, body), nil
			})
			got := s.archive(context.Background(), site)
			wantHost := alternate
			if mixed {
				wantHost = requested
			}
			if got.Status != "connected" || got.ReportingHost != wantHost || calls != 1 || got.Truncated != mixed || len(got.Captures) == 0 {
				t.Fatalf("incorrect canonical host evidence: %+v, calls %d", got, calls)
			}
			for _, capture := range got.Captures {
				if !strings.HasPrefix(capture.URL, "https://"+wantHost+"/") {
					t.Fatal("mixed reporting host", capture)
				}
			}
		}
	}
}
