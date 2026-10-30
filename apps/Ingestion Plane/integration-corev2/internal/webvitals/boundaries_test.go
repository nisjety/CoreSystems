package webvitals

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/cookiejar"
	"os"
	"strings"
	"sync"
	"testing"
	"time"
)

func serviceWithPayload(t *testing.T, raw string) *Service {
	t.Helper()
	s, err := NewService(manifest(t, nil), "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(raw))}, nil
	})}, func() time.Time { return testNow })
	if err != nil {
		t.Fatal(err)
	}
	return s
}
func TestExportMetricsPrivacyAndFreshnessBoundaries(t *testing.T) {
	for _, sample := range []string{
		strings.Replace(validSample, `"value":1324.5,`, "", 1),
		strings.Replace(validSample, `1324.5`, `null`, 1),
		strings.Replace(validSample, `1324.5`, `-1`, 1),
		strings.Replace(validSample, `"sequence":1`, `"sequence":33`, 1),
		strings.Replace(validSample, `"wide"`, `"mobile"`, 1),
		strings.Replace(validSample, `"LCP"`, `"FCP"`, 1),
		strings.Replace(validSample, `"LCP"`, `"INP"`, 1), // Value is valid; accepted below.
	} {
		s := serviceWithPayload(t, reportJSON(sample))
		_, err := s.Get(context.Background(), "org-1", input())
		if strings.Contains(sample, `"metric":"INP"`) {
			if err != nil {
				t.Fatal(err)
			}
		} else if err == nil {
			t.Fatalf("invalid export accepted: %s", sample)
		}
	}
	for _, metric := range []string{"INP", "CLS"} {
		sample := strings.Replace(strings.Replace(validSample, `"LCP"`, `"`+metric+`"`, 1), `1324.5`, `0`, 1)
		if _, err := serviceWithPayload(t, reportJSON(sample)).Get(context.Background(), "org-1", input()); err != nil {
			t.Fatal("real zero rejected", err)
		}
	}
	for _, raw := range []string{
		`{}`, `null`, reportJSON("") + `{}`,
		strings.Replace(reportJSON(validSample), `"success":true`, `"success":false`, 1),
		strings.Replace(reportJSON(validSample), `"websiteRevision":7`, `"websiteRevision":8`, 1),
		strings.Replace(reportJSON(validSample), `"status":"connected"`, `"status":"not_configured"`, 1),
		strings.Replace(reportJSON(validSample), `"observedAt":"2026-10-23T12:00:00Z"`, `"observedAt":"2026-10-23T12:00:01Z"`, 1),
		strings.Replace(reportJSON(validSample), `"observedAt":"2026-10-23T12:00:00Z"`, `"observedAt":"2026-10-23T11:00:00Z"`, 1),
		strings.Replace(reportJSON(validSample), `"samples":[`+validSample+`]`, `"samples":null`, 1),
		strings.Repeat("x", (2<<20)+1),
	} {
		if _, err := serviceWithPayload(t, raw).Get(context.Background(), "org-1", input()); err == nil {
			t.Fatal("invalid report accepted")
		}
	}
	if report, err := serviceWithPayload(t, reportJSON("")).Get(context.Background(), "org-1", input()); err != nil || report.Status != "connected" || len(report.Samples) != 0 {
		t.Fatal("valid measured empty export rejected")
	}
	clean := strings.Replace(validSample, `"value":1324.5`, `"value":1324.5,"email":"should-not-cross"`, 1)
	report, err := serviceWithPayload(t, reportJSON(clean)).Get(context.Background(), "org-1", input())
	if err != nil {
		t.Fatal(err)
	}
	out, _ := json.Marshal(report)
	if strings.Contains(string(out), "should-not-cross") {
		t.Fatal("unknown personal field forwarded")
	}
}
func TestExportUsesSevenUTCCalendarDaysAndFiveMinuteFreshness(t *testing.T) {
	for _, tc := range []struct {
		received, observed string
		valid              bool
	}{
		{"2026-10-17T00:00:00Z", "2026-10-23T12:00:00Z", true},
		{"2026-10-16T23:59:59Z", "2026-10-23T12:00:00Z", false},
		{"2026-10-23T11:00:00Z", "2026-10-23T11:55:00Z", true},
		{"2026-10-23T11:00:00Z", "2026-10-23T11:54:59Z", false},
	} {
		sample := strings.Replace(validSample, "2026-10-23T11:00:00Z", tc.received, 1)
		raw := strings.Replace(reportJSON(sample), `"observedAt":"2026-10-23T12:00:00Z"`, `"observedAt":"`+tc.observed+`"`, 1)
		_, err := serviceWithPayload(t, raw).Get(context.Background(), "org-1", input())
		if (err == nil) != tc.valid {
			t.Fatal(tc, err)
		}
	}
}
func TestSharedPageMetricIDsAndContext(t *testing.T) {
	inp := strings.Replace(validSample, `"LCP"`, `"INP"`, 1)
	if _, err := serviceWithPayload(t, reportJSON(validSample+","+inp)).Get(context.Background(), "org-1", input()); err != nil {
		t.Fatal("shared page UUID must support several metrics", err)
	}
	changed := strings.Replace(inp, `"path":"/"`, `"path":"/innsikt"`, 1)
	if _, err := serviceWithPayload(t, reportJSON(validSample+","+changed)).Get(context.Background(), "org-1", input()); err == nil {
		t.Fatal("page context changed")
	}
}
func TestDevelopmentBindingIsExplicitAndCannotSelectAnyOtherHost(t *testing.T) {
	for _, backend := range []string{"", "http://host.docker.internal:3000"} {
		path := manifest(t, func(m map[string]any) {
			m["environment"] = "development"
			m["reportingOrigin"] = "http://localhost:3000"
			m["developmentBackendOrigin"] = backend
		})
		if _, err := NewService(path, "production", nil, nil); err == nil {
			t.Fatal("production enabled development")
		}
		s, err := NewService(path, "development", &http.Client{Transport: roundTrip(func(r *http.Request) (*http.Response, error) {
			want := backend
			if want == "" {
				want = "http://localhost:3000"
			}
			if r.URL.String() != want+"/api/web-vitals/export" {
				t.Fatal("unexpected development destination")
			}
			raw := strings.ReplaceAll(reportJSON(validSample), `"production"`, `"development"`)
			raw = strings.ReplaceAll(raw, "https://www.triodelab.no", "http://localhost:3000")
			return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(raw))}, nil
		})}, func() time.Time { return testNow })
		if err != nil {
			t.Fatal(err)
		}
		in := input()
		in.Environment = "development"
		if _, err := s.Get(context.Background(), "org-1", in); err != nil {
			t.Fatal(err)
		}
	}
	for _, backend := range []string{"http://169.254.169.254", "http://localhost:3026", "http://host.docker.internal:3026", "https://triodelab.no"} {
		if _, err := NewService(manifest(t, func(m map[string]any) {
			m["environment"] = "development"
			m["reportingOrigin"] = "http://localhost:3000"
			m["developmentBackendOrigin"] = backend
		}), "development", nil, nil); err == nil {
			t.Fatal("development widened")
		}
	}
}
func TestLocalRuntimeSupportsOnlyTheExplicitDevelopmentException(t *testing.T) {
	path := manifest(t, func(m map[string]any) {
		m["environment"] = "development"
		m["reportingOrigin"] = "http://localhost:3000"
		m["developmentBackendOrigin"] = "http://host.docker.internal:3000"
	})
	if _, err := NewService(path, "local", nil, nil); err != nil {
		t.Fatal("canonical local runtime rejected", err)
	}
	for _, runtime := range []string{"production", "staging", "test", ""} {
		if _, err := NewService(path, runtime, nil, nil); err == nil {
			t.Fatal("non-local runtime accepted development exception", runtime)
		}
	}
}
func TestTransportFailureRedirectAndRevocationFailClosed(t *testing.T) {
	for _, code := range []int{301, 302, 401, 404, 429, 500} {
		s, err := NewService(manifest(t, nil), "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: code, Header: http.Header{"Location": []string{"https://evil.example"}}, Body: io.NopCloser(strings.NewReader("secret upstream failure"))}, nil
		})}, func() time.Time { return testNow })
		if err != nil {
			t.Fatal(err)
		}
		if _, err := s.Get(context.Background(), "org-1", input()); err != ErrUnavailable {
			t.Fatal("provider detail or false data")
		}
	}
	s, err := NewService(manifest(t, nil), "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) { return nil, errors.New("sensitive transport") })}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.Get(context.Background(), "org-1", input()); err != ErrUnavailable {
		t.Fatal("transport error leaked")
	}
	path := manifest(t, nil)
	s, err = NewService(path, "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
		if err := os.Remove(path); err != nil {
			t.Fatal(err)
		}
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(reportJSON(validSample)))}, nil
	})}, func() time.Time { return testNow })
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.Get(context.Background(), "org-1", input()); err != ErrUnavailable {
		t.Fatal("revoked binding remained authoritative")
	}
	jar, _ := cookiejar.New(nil)
	original := &http.Client{Jar: jar, Timeout: time.Hour}
	closed := closedClient(original)
	if closed.Jar != nil || closed.Timeout != 15*time.Second || closed.CheckRedirect(nil, nil) != http.ErrUseLastResponse || original.Jar != jar || original.Timeout != time.Hour {
		t.Fatal("client boundary or mutation")
	}
}
func TestAdmissionIsGloballyBounded(t *testing.T) {
	started := make(chan struct{}, 2)
	release := make(chan struct{})
	var wg sync.WaitGroup
	s, err := NewService(manifest(t, nil), "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
		started <- struct{}{}
		<-release
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(reportJSON(validSample)))}, nil
	})}, func() time.Time { return testNow })
	if err != nil {
		t.Fatal(err)
	}
	for range 2 {
		wg.Add(1)
		go func() { defer wg.Done(); _, _ = s.Get(context.Background(), "org-1", input()) }()
	}
	<-started
	<-started
	if _, err := s.Get(context.Background(), "org-1", input()); err != ErrBusy {
		t.Fatal("admission not bounded")
	}
	close(release)
	wg.Wait()
}
func TestStrictRequestAndJSONScanner(t *testing.T) {
	valid := `{"websiteId":"` + testWebsite + `","site":"https://triodelab.no/","websiteRevision":7,"environment":"production"}`
	for _, raw := range []string{`null`, `[]`, `{}`, valid + `{}`, strings.Replace(valid, `"websiteId"`, `"WebsiteID"`, 1), strings.Replace(valid, `"websiteRevision":7`, `"websiteRevision":7,"websiteRevision":8`, 1), strings.Replace(valid, `"websiteRevision":7`, `"websiteRevision":"7"`, 1), strings.Replace(valid, `"environment":"production"`, `"environment":"staging"`, 1), valid + strings.Repeat(" ", 2048)} {
		if _, err := ParseInput([]byte(raw)); err == nil {
			t.Fatal("bad input accepted")
		}
	}
	if _, err := ParseInput([]byte(valid)); err != nil {
		t.Fatal(err)
	}
	for _, raw := range []string{`{"x":{"a":1,"a":2}}`, `{"x":]}`, `[}`, strings.Repeat("[", 18) + strings.Repeat("]", 18)} {
		if UniqueJSON([]byte(raw)) == nil {
			t.Fatal("bad JSON accepted")
		}
	}
	if _, err := serviceWithPayload(t, reportJSON(validSample)).Get(context.Background(), "", input()); err != ErrInvalid {
		t.Fatal("missing org accepted")
	}
}

func TestDevelopmentPortPairMustMatchAndNeverWorksInProduction(t *testing.T) {
	b := binding{Environment: "development", ReportingOrigin: "http://localhost:3001", DevelopmentBackendOrigin: "http://host.docker.internal:3001"}
	if !validOrigin(b, "development") || validOrigin(b, "production") {
		t.Fatal("development port boundary changed")
	}
	b.DevelopmentBackendOrigin = "http://host.docker.internal:3000"
	if validOrigin(b, "development") {
		t.Fatal("mismatched dev backend port accepted")
	}
	b.ReportingOrigin = "http://localhost:3002"
	b.DevelopmentBackendOrigin = "http://host.docker.internal:3002"
	if validOrigin(b, "development") {
		t.Fatal("arbitrary loopback port accepted")
	}
}
