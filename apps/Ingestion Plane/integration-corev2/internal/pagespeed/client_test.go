package pagespeed

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

type transportFunc func(*http.Request) (*http.Response, error)

func (f transportFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

const fixture = `{"lighthouseResult":{"requestedUrl":"https://aquatiq.com/","finalUrl":"https://www.aquatiq.com/no","configSettings":{"formFactor":"mobile"},"fetchTime":"2026-10-21T12:00:00.000Z","lighthouseVersion":"13.0.0","categories":{"performance":{"score":0.79},"accessibility":{"score":1},"best-practices":{"score":0.9},"seo":{"score":null}},"audits":{"largest-contentful-paint":{"numericValue":2345},"cumulative-layout-shift":{"numericValue":0.07},"total-blocking-time":{"numericValue":130},"first-contentful-paint":{"numericValue":1100},"speed-index":{"numericValue":1800},"unprojected-audit":{"numericValue":-3}},"fullPageScreenshot":{"screenshot":{"data":"DO_NOT_RETAIN"}}},"loadingExperience":{"metrics":{"INTERACTION_TO_NEXT_PAINT":{"percentile":777}}}}`

func TestClientFixedEndpointAndProjection(t *testing.T) {
	client := NewClient("synthetic-key", &http.Client{Transport: transportFunc(func(r *http.Request) (*http.Response, error) {
		if r.URL.Scheme != "https" || r.URL.Host != "www.googleapis.com" || r.URL.Path != "/pagespeedonline/v5/runPagespeed" || r.Method != http.MethodGet {
			t.Fatalf("unexpected destination %s", r.URL.Redacted())
		}
		query := r.URL.Query()
		if query.Get("url") != "https://aquatiq.com/" || query.Get("strategy") != "mobile" || query.Get("key") != "synthetic-key" || len(query["category"]) != 4 {
			t.Fatal("unexpected provider query")
		}
		if r.Header.Get("Authorization") != "" {
			t.Fatal("OAuth must not be sent")
		}
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": {"application/json"}}, Body: io.NopCloser(strings.NewReader(fixture))}, nil
	})})
	report, reason := client.Run(context.Background(), "https://aquatiq.com/", "mobile")
	if reason != "" || report == nil || *report.Performance != 79 || *report.Accessibility != 100 || report.SEO != nil || *report.LCPMs != 2345 || *report.TBTMs != 130 {
		t.Fatalf("projection = %+v, reason %q", report, reason)
	}
}

func TestClientFailuresAreBoundedAndSanitized(t *testing.T) {
	for _, tc := range []struct {
		name, body string
		status     int
		err        error
		reason     string
	}{
		{"quota", "secret provider detail", 429, nil, "quota_limited"},
		{"configuration", "secret provider detail", 403, nil, "not_configured"},
		{"upstream", "secret provider detail", 502, nil, "source_unavailable"},
		{"redirect", "secret provider detail", 302, nil, "source_unavailable"},
		{"network", "", 0, errors.New("secret key url"), "source_unavailable"},
		{"json", "not json", 200, nil, "audit_failed"},
		{"empty", `{}`, 200, nil, "audit_failed"},
		{"runtime", `{"lighthouseResult":{"runtimeError":{"code":"NO_FCP"}}}`, 200, nil, "audit_failed"},
		{"huge", strings.Repeat("x", (5<<20)+1), 200, nil, "audit_failed"},
		{"scores", `{"lighthouseResult":{"fetchTime":"2026-10-21T12:00:00Z","lighthouseVersion":"13","categories":{"performance":{"score":1.1}}}}`, 200, nil, "audit_failed"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			client := NewClient("", &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
				if tc.err != nil {
					return nil, tc.err
				}
				return &http.Response{StatusCode: tc.status, Header: http.Header{}, Body: io.NopCloser(strings.NewReader(tc.body))}, nil
			})})
			report, reason := client.Run(context.Background(), "https://aquatiq.com/", "mobile")
			if report != nil || reason != tc.reason {
				t.Fatalf("report %+v reason %q", report, reason)
			}
		})
	}
}

func TestInputValidation(t *testing.T) {
	for _, site := range []string{"https://aquatiq.com/", "https://www.aquatiq.com/"} {
		if ValidateInput(site, "desktop") != nil {
			t.Fatalf("valid site rejected: %s", site)
		}
	}
	for _, site := range []string{"http://aquatiq.com/", "https://aquatiq.com", "https://AQUATIQ.com/", "https://aquatiq.com/page", "https://aquatiq.com/?x=1", "https://aquatiq.com/#x", "https://user@aquatiq.com/", "https://aquatiq.com:443/", "https://localhost/", "https://test.local/", "https://metadata.google.internal/", "https://127.0.0.1/", "https://[::1]/", "https://169.254.169.254/", "https://aquatiq.com./", "https://bad_.com/", " https://aquatiq.com/", "https://aquatiq.com/%2f", "https://aquatiq.com/?"} {
		if ValidateInput(site, "mobile") == nil {
			t.Fatalf("unsafe site accepted: %s", site)
		}
	}
	if ValidateInput("https://aquatiq.com/", "arbitrary") == nil {
		t.Fatal("unknown strategy accepted")
	}
}

func TestLighthouseBindingValidationAndNullMeasurements(t *testing.T) {
	for _, tc := range []struct{ name, from, to string }{
		{"requested-site", `"requestedUrl":"https://aquatiq.com/"`, `"requestedUrl":"https://other.com/"`},
		{"form-factor", `"formFactor":"mobile"`, `"formFactor":"desktop"`},
		{"bad-score", `"score":0.79`, `"score":1.1`},
		{"negative-metric", `"numericValue":2345`, `"numericValue":-1`},
		{"bad-time", `2026-10-21T12:00:00.000Z`, `not-a-time`},
		{"bad-version", `13.0.0`, `<unsafe>`},
		{"runtime-error", `"requestedUrl"`, `"runtimeError":{"code":"NO_FCP"},"requestedUrl"`},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if report, reason := decodeReport([]byte(strings.Replace(fixture, tc.from, tc.to, 1)), "https://aquatiq.com/", "mobile"); report != nil || reason != "audit_failed" {
				t.Fatalf("invalid report admitted: %+v %s", report, reason)
			}
		})
	}
	legacy := strings.Replace(fixture, `"formFactor":"mobile"`, `"emulatedFormFactor":"mobile"`, 1)
	if report, reason := decodeReport([]byte(legacy), "https://aquatiq.com/", "mobile"); report == nil || reason != "" {
		t.Fatalf("legacy form factor rejected: %s", reason)
	}
	raw := `{"lighthouseResult":{"requestedUrl":"https://aquatiq.com/","configSettings":{"formFactor":"mobile"},"fetchTime":"2026-10-21T12:00:00Z","lighthouseVersion":"13","categories":{"performance":{"score":0}}}}`
	if report, reason := decodeReport([]byte(raw), "https://aquatiq.com/", "mobile"); reason != "" || report.Performance == nil || *report.Performance != 0 || report.LCPMs != nil || report.Accessibility != nil {
		t.Fatalf("missing values not preserved: %+v %s", report, reason)
	}
	client := NewClient("", nil)
	if client.http.Timeout != 75*time.Second || client.http.Transport == nil || client.http.CheckRedirect(nil, nil) != http.ErrUseLastResponse {
		t.Fatal("default client is not bounded")
	}
	if report, reason := client.Run(context.Background(), "https://localhost/", "mobile"); report != nil || reason != "audit_failed" {
		t.Fatal("client allowed unsafe target")
	}
}
