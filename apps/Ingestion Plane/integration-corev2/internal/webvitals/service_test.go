package webvitals

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const testWebsite = "web_12345678901234567890123456789012"

var testNow = time.Date(2026, 10, 23, 12, 0, 0, 0, time.UTC)

type roundTrip func(*http.Request) (*http.Response, error)

func (f roundTrip) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func manifest(t *testing.T, mutate func(map[string]any)) string {
	t.Helper()
	dir := t.TempDir()
	token := filepath.Join(dir, "token")
	if err := os.WriteFile(token, []byte(strings.Repeat("t", 43)), 0600); err != nil {
		t.Fatal(err)
	}
	item := map[string]any{"organizationId": "org-1", "websiteId": testWebsite, "websiteRevision": 7, "site": "https://triodelab.no/", "environment": "production", "reportingOrigin": "https://www.triodelab.no", "tokenFile": token, "paths": []string{"/", "/innsikt"}}
	if mutate != nil {
		mutate(item)
	}
	raw, _ := json.Marshal(map[string]any{"version": 1, "installations": []any{item}})
	path := filepath.Join(dir, "bindings.json")
	if err := os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	return path
}
func input() Input {
	return Input{WebsiteID: testWebsite, Site: "https://triodelab.no/", WebsiteRevision: 7, Environment: "production"}
}
func reportJSON(sample string) string {
	return `{"success":true,"data":{"websiteId":"` + testWebsite + `","site":"https://triodelab.no/","websiteRevision":7,"environment":"production","reportingOrigin":"https://www.triodelab.no","status":"connected","samples":[` + sample + `],"observedAt":"2026-10-23T12:00:00Z"}}`
}

const validSample = `{"sampleId":"12345678-1234-4234-8234-123456789012","sequence":1,"metric":"LCP","value":1324.5,"viewport":"wide","path":"/","receivedAt":"2026-10-23T11:00:00Z"}`

func TestMissingBindingDoesNotFetchOrManufactureMeasurements(t *testing.T) {
	s, err := NewService("", "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) { t.Fatal("unexpected fetch"); return nil, nil })}, func() time.Time { return testNow })
	if err != nil {
		t.Fatal(err)
	}
	r, err := s.Get(context.Background(), "org-1", input())
	if err != nil || r.Status != "not_configured" || r.Samples == nil || len(r.Samples) != 0 || r.ReportingOrigin != "" {
		t.Fatalf("unexpected report %+v %v", r, err)
	}
}
func TestExportUsesOnlyExactBoundOriginAndCredential(t *testing.T) {
	path := manifest(t, nil)
	called := 0
	client := &http.Client{Transport: roundTrip(func(req *http.Request) (*http.Response, error) {
		called++
		if req.Method != "GET" || req.URL.String() != "https://www.triodelab.no/api/web-vitals/export" || req.Header.Get("Authorization") != "Bearer "+strings.Repeat("t", 43) || req.Header.Get("Cookie") != "" || req.Header.Get("X-Org-ID") != "" {
			t.Fatal("credential or endpoint boundary")
		}
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(reportJSON(validSample)))}, nil
	})}
	s, err := NewService(path, "production", client, func() time.Time { return testNow })
	if err != nil {
		t.Fatal(err)
	}
	r, err := s.Get(context.Background(), "org-1", input())
	if err != nil || r.Status != "connected" || len(r.Samples) != 1 || r.Samples[0].Value != 1324.5 {
		t.Fatalf("%+v %v", r, err)
	}
	for _, tc := range []struct {
		org string
		in  Input
	}{{"org-other", input()}, {"org-1", Input{WebsiteID: testWebsite, Site: "https://triodelab.no/", WebsiteRevision: 8, Environment: "production"}}} {
		got, err := s.Get(context.Background(), tc.org, tc.in)
		if err != nil || got.Status != "not_configured" {
			t.Fatal("cross scope")
		}
	}
	if called != 1 {
		t.Fatal("cross-scope fetched")
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Get(context.Background(), "org-1", input()); err == nil {
		t.Fatal("missing manifest accepted")
	}
}
func TestManifestRejectsUnsafeBindingsAndPermissions(t *testing.T) {
	for _, tc := range []struct {
		name   string
		mutate func(map[string]any)
	}{
		{"other-host", func(m map[string]any) { m["reportingOrigin"] = "https://evil.example" }},
		{"http", func(m map[string]any) { m["reportingOrigin"] = "http://triodelab.no" }},
		{"query", func(m map[string]any) { m["reportingOrigin"] = "https://triodelab.no?token=x" }},
		{"path", func(m map[string]any) { m["reportingOrigin"] = "https://triodelab.no/private" }},
		{"private-path", func(m map[string]any) { m["paths"] = []string{"/account"} }},
		{"parameter-path", func(m map[string]any) { m["paths"] = []string{"/innsikt?user=secret"} }},
		{"unknown", func(m map[string]any) { m["apiKey"] = "synthetic" }},
		{"case-alias", func(m map[string]any) { m["WebsiteID"] = m["websiteId"]; delete(m, "websiteId") }},
		{"dev-in-production", func(m map[string]any) {
			m["environment"] = "development"
			m["reportingOrigin"] = "http://localhost:3000"
		}},
		{"dev-override-production", func(m map[string]any) { m["developmentBackendOrigin"] = "http://host.docker.internal:3000" }},
		{"revision", func(m map[string]any) { m["websiteRevision"] = 0 }},
	} {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := NewService(manifest(t, tc.mutate), "production", nil, nil); err == nil {
				t.Fatal("unsafe manifest accepted")
			}
		})
	}
	path := manifest(t, nil)
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := NewService(path, "production", nil, nil); err == nil {
		t.Fatal("public manifest")
	}
}
func TestMalformedUpstreamIsUnavailableNeverEmptySuccess(t *testing.T) {
	for _, sample := range []string{
		strings.Replace(validSample, `"/"`, `"/account"`, 1),
		strings.Replace(validSample, `1324.5`, `120001`, 1),
		strings.Replace(validSample, `2026-10-23T11:00:00Z`, `2026-10-24T11:00:00Z`, 1),
		strings.Replace(validSample, `2026-10-23T11:00:00Z`, `2026-10-01T11:00:00Z`, 1),
		validSample + "," + validSample,
	} {
		s, err := NewService(manifest(t, nil), "production", &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(reportJSON(sample)))}, nil
		})}, func() time.Time { return testNow })
		if err != nil {
			t.Fatal(err)
		}
		if _, err := s.Get(context.Background(), "org-1", input()); err == nil {
			t.Fatal("invalid export accepted")
		}
	}
}
func TestManifestRejectsExportCredentialReuseAcrossInstallations(t *testing.T) {
	path := manifest(t, nil)
	raw, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var value struct {
		Version       int              `json:"version"`
		Installations []map[string]any `json:"installations"`
	}
	if err = json.Unmarshal(raw, &value); err != nil {
		t.Fatal(err)
	}
	second := map[string]any{}
	for k, v := range value.Installations[0] {
		second[k] = v
	}
	second["environment"] = "development"
	second["reportingOrigin"] = "http://localhost:3000"
	value.Installations = append(value.Installations, second)
	raw, _ = json.Marshal(value)
	if err = os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = NewService(path, "development", nil, nil); err == nil {
		t.Fatal("shared installation export credential accepted")
	}
	separate := filepath.Join(filepath.Dir(path), "other-token")
	if err = os.WriteFile(separate, []byte(strings.Repeat("u", 43)), 0600); err != nil {
		t.Fatal(err)
	}
	second["tokenFile"] = separate
	raw, _ = json.Marshal(value)
	if err = os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = NewService(path, "development", nil, nil); err != nil {
		t.Fatal("distinct installation export credential rejected", err)
	}
}
