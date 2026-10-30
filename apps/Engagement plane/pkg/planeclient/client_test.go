package planeclient

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type transportFunc func(*http.Request) (*http.Response, error)

func (f transportFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func fixtureToken(t *testing.T, audience string, scopes ...string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "bearer")
	writeClaims(t, path, map[string]any{"aud": audience, "org_id": "org-test", "sub": "service:marketing-core", "service_id": "service:marketing-core", "principal_type": "service", "exp": time.Now().Add(time.Hour).Unix(), "scopes": scopes})
	return path
}
func writeClaims(t *testing.T, path string, claims map[string]any) {
	t.Helper()
	raw, _ := json.Marshal(claims)
	token := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"RS256"}`)) + "." + base64.RawURLEncoding.EncodeToString(raw) + ".c2ln"
	if err := os.WriteFile(path, []byte(token), 0600); err != nil {
		t.Fatal(err)
	}
}
func fixtureConfig(t *testing.T) Config {
	return Config{Core: "marketing-core", OrganizationID: "org-test", QuarryURL: "https://quarry.example", QuarryTokenFile: fixtureToken(t, "quarry", "map:read", "scrape:read", "scrape:write"), ModelURL: "https://model.example", ModelTokenFile: fixtureToken(t, "model-gateway", "models:invoke"), InferenceTokenFile: fixtureToken(t, "inference-core", "inference:invoke")}
}
func response(body string, status int) *http.Response {
	return &http.Response{StatusCode: status, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(body))}
}

func TestDiscoveryExactContract(t *testing.T) {
	cfg := fixtureConfig(t)
	calls := 0
	c, err := New(cfg, &http.Client{Transport: transportFunc(func(r *http.Request) (*http.Response, error) {
		calls++
		if r.URL.String() != "https://quarry.example/v1/map" || r.Method != "POST" {
			t.Fatal("unexpected destination")
		}
		if r.Header.Get("Authorization") == "" {
			t.Fatal("missing bearer")
		}
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if len(body) != 3 || body["url"] != "https://triodelab.no/" || body["limit"] != float64(25) || body["include_subdomains"] != false {
			t.Fatalf("bad body %v", body)
		}
		return response(`{"url":"https://triodelab.no/","links":[{"url":"https://triodelab.no/innsikt","title":"Innsikt","secret":"private"}],"count":1,"ranked":false}`, 200), nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	result, err := c.Discover(context.Background(), "https://triodelab.no/")
	if err != nil || len(result.Pages) != 1 || calls != 1 {
		t.Fatalf("discovery failed %#v %v", result, err)
	}
}
func TestDraftUsesPairOfAudienceBoundCredentialsAndNoTools(t *testing.T) {
	cfg := fixtureConfig(t)
	calls := 0
	c, _ := New(cfg, &http.Client{Transport: transportFunc(func(r *http.Request) (*http.Response, error) {
		calls++
		if r.URL.Path != "/v1/ai/chat" || r.URL.Host != "model.example" {
			t.Fatal("wrong model endpoint")
		}
		if r.Header.Get("Authorization") == r.Header.Get("X-Inference-Authorization") || r.Header.Get("X-Inference-Authorization") == "" {
			t.Fatal("distinct credentials required")
		}
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if body["zdr"] != true || body["stream"] != false || body["tools"] != nil {
			t.Fatalf("bad draft policy %v", body)
		}
		messages := body["messages"].([]any)
		if len(messages) != 2 || messages[0].(map[string]any)["role"] != "system" {
			t.Fatal("server policy missing")
		}
		return response(`{"id":"draft-1","content":"Draft content","model_used":"model-a","usage":{"input_tokens":10,"output_tokens":2},"token":"private"}`, 200), nil
	})})
	result, err := c.Draft(context.Background(), "Write a title.")
	if err != nil || result.Content != "Draft content" || calls != 1 {
		t.Fatalf("draft failed %#v %v", result, err)
	}
	cfg.InferenceTokenFile = cfg.ModelTokenFile
	c, _ = New(cfg, nil)
	_, err = c.Draft(context.Background(), "x")
	if !errors.Is(err, ErrUnavailable) {
		t.Fatalf("shared credential accepted %v", err)
	}
}
func TestCaptureZDRAndIngestAuthority(t *testing.T) {
	cfg := fixtureConfig(t)
	calls := 0
	c, _ := New(cfg, &http.Client{Transport: transportFunc(func(r *http.Request) (*http.Response, error) {
		calls++
		var body map[string]any
		_ = json.NewDecoder(r.Body).Decode(&body)
		if r.URL.Path != "/v1/scrape" || body["zdr"] != true || body["ingest"] != false || len(body) != 3 {
			t.Fatalf("bad capture %v", body)
		}
		return response(`{"request_id":"req_test","data":{"run_id":"run_test","url":{"requested":"https://triodelab.no/innsikt","final_url":"https://triodelab.no/innsikt"},"status":200,"fetched_at":"2026-10-22T12:00:00Z","fingerprint":"blake3:test","formats":{"markdown":{"artifact_id":"art_test","bytes":10}},"token":"private"},"error":null}`, 200), nil
	})})
	result, err := c.Capture(context.Background(), "https://triodelab.no/innsikt", false, true)
	if err != nil || result.RunID != "run_test" || calls != 1 {
		t.Fatalf("capture failed %#v %v", result, err)
	}
	_, err = c.Capture(context.Background(), "https://triodelab.no/", true, true)
	if !errors.Is(err, ErrInvalidInput) || calls != 1 {
		t.Fatal("ZDR ingest accepted")
	}
	cfg.QuarryTokenFile = fixtureToken(t, "quarry", "scrape:read")
	c, _ = New(cfg, nil)
	_, err = c.Capture(context.Background(), "https://triodelab.no/", true, false)
	if !errors.Is(err, ErrUnavailable) {
		t.Fatal("ingest accepted read-only bearer")
	}
}
func TestNoNetworkOnDisabledReadinessAndInvalidInput(t *testing.T) {
	c, err := New(Config{Core: "marketing-core"}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if c.Readiness().Quarry.Status != "disabled" || c.Readiness().Model.Status != "disabled" {
		t.Fatal("misleading readiness")
	}
	_, err = c.Draft(context.Background(), "x")
	if !errors.Is(err, ErrUnavailable) {
		t.Fatal("disabled draft allowed")
	}
	cfg := fixtureConfig(t)
	calls := 0
	c, _ = New(cfg, &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) { calls++; return nil, errors.New("private") })})
	for _, u := range []string{"http://triodelab.no/", "https://localhost/", "https://127.0.0.1/", "https://x.internal/", "https://user@triodelab.no/", "https://triodelab.no/?token=x", "https://triodelab.no/%0afoo", "https://triodelab.no/admin", "https://triodelab.no/%2561dmin", "https://triodelab.no/%252e%252e/admin", "https://triodelab.no/x#", "https://triodelab.no/with space"} {
		if _, err = c.Discover(context.Background(), u); !errors.Is(err, ErrInvalidInput) {
			t.Fatalf("accepted unsafe %q", u)
		}
	}
	if _, err = c.Draft(context.Background(), strings.Repeat("x", 8193)); !errors.Is(err, ErrInvalidInput) {
		t.Fatal("oversized prompt accepted")
	}
	if calls != 0 {
		t.Fatal("invalid input made network call")
	}
	if c.Readiness().Model.Status != "configured" {
		t.Fatal("valid file not configured")
	}
	if calls != 0 {
		t.Fatal("readiness called network")
	}
}
