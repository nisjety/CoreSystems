package planeclient

import (
	"context"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestConfigurationAndEnvironment(t *testing.T) {
	for key, value := range map[string]string{"STUDIO_ORGANIZATION_ID": "org-test", "QUARRY_URL": "http://quarry-edge:8082", "QUARRY_BEARER_TOKEN_FILE": "/run/secrets/quarry", "MODEL_GATEWAY_URL": "http://model-gateway:8080", "MODEL_BEARER_TOKEN_FILE": "/run/secrets/model", "INFERENCE_BEARER_TOKEN_FILE": "/run/secrets/inference"} {
		t.Setenv(key, value)
	}
	cfg := ConfigFromEnvironment("marketing-core")
	c, err := New(cfg, nil)
	if err != nil {
		t.Fatal(err)
	}
	if c.Readiness().Model.Status != "missing_credential" || c.Readiness().Quarry.Status != "missing_credential" {
		t.Fatal("missing credentials marked configured")
	}
	for _, bad := range []Config{{Core: "marketing-core", QuarryURL: "https://quarry.example"}, {Core: "bad core"}, {Core: "marketing-core", QuarryTokenFile: "/run/secrets/x"}, {Core: "marketing-core", ModelTokenFile: "/run/secrets/x"}, {Core: "marketing-core", OrganizationID: "org-test", QuarryURL: "http://evil.example"}, {Core: "marketing-core", OrganizationID: "org-test", QuarryURL: "https://user@quarry.example"}, {Core: "marketing-core", OrganizationID: "org-test", QuarryURL: "https://quarry.example/path"}, {Core: "marketing-core", OrganizationID: "org-test", QuarryURL: "https://quarry.example", QuarryTokenFile: "relative"}} {
		if _, err = New(bad, nil); err == nil {
			t.Fatal("invalid configuration accepted")
		}
	}
	clone := http.DefaultTransport.(*http.Transport).Clone()
	clone.Proxy = http.ProxyFromEnvironment
	c, err = New(fixtureConfig(t), &http.Client{Transport: clone, Jar: nil})
	if err != nil || c.http.Transport.(*http.Transport).Proxy != nil {
		t.Fatal("proxy inherited")
	}
	c, err = New(fixtureConfig(t), &http.Client{})
	if err != nil || c.http.Transport == nil {
		t.Fatal("default transport not hardened")
	}
}

func TestCredentialAssertionsPrivateFreshAndBound(t *testing.T) {
	cfg := fixtureConfig(t)
	c, _ := New(cfg, nil)
	valid := map[string]any{"aud": "quarry", "org_id": "org-test", "sub": "service:marketing-core", "service_id": "service:marketing-core", "principal_type": "service", "exp": time.Now().Add(time.Hour).Unix(), "scopes": []string{"map:read"}}
	for key, bad := range map[string]any{"aud": "model-gateway", "org_id": "org-other", "sub": "service:social-core", "service_id": "service:social-core", "principal_type": "user", "user_id": "someone", "exp": time.Now().Unix(), "nbf": time.Now().Add(time.Hour).Unix(), "scopes": []string{"scrape:read"}} {
		claims := make(map[string]any, len(valid)+1)
		for k, v := range valid {
			claims[k] = v
		}
		claims[key] = bad
		writeClaims(t, cfg.QuarryTokenFile, claims)
		if _, err := c.bearer(cfg.QuarryTokenFile, "quarry", "map:read"); !errors.Is(err, ErrUnavailable) {
			t.Fatalf("bad %s accepted", key)
		}
	}
	writeClaims(t, cfg.QuarryTokenFile, valid)
	if _, err := c.bearer(cfg.QuarryTokenFile, "quarry", "map:read"); err != nil {
		t.Fatal("fresh credential not reopened")
	}
	if err := os.Chmod(cfg.QuarryTokenFile, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := c.bearer(cfg.QuarryTokenFile, "quarry", "map:read"); !errors.Is(err, ErrUnavailable) {
		t.Fatal("world readable file accepted")
	}
	if err := os.Chmod(cfg.QuarryTokenFile, 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(t.TempDir(), "link")
	if err := os.Symlink(cfg.QuarryTokenFile, link); err != nil {
		t.Fatal(err)
	}
	for _, p := range []string{link, t.TempDir(), "", filepath.Join(t.TempDir(), "missing")} {
		if _, err := c.bearer(p, "quarry", "map:read"); !errors.Is(err, ErrUnavailable) {
			t.Fatal("unsafe file accepted")
		}
	}
	for _, raw := range []string{"not-a-token", "YQ.Yg.c2ln", "eyJhbGciOiJub25lIn0.e30.c2ln", strings.Repeat("x", 8193)} {
		if err := os.WriteFile(cfg.QuarryTokenFile, []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := c.bearer(cfg.QuarryTokenFile, "quarry", "map:read"); !errors.Is(err, ErrUnavailable) {
			t.Fatal("malformed credential accepted")
		}
	}
}

func TestOwnerFailuresAreBoundedSanitizedAndNeverRetried(t *testing.T) {
	for _, status := range []int{301, 401, 403, 429, 500} {
		t.Run(http.StatusText(status), func(t *testing.T) {
			calls := 0
			c, _ := New(fixtureConfig(t), &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
				calls++
				return response(`{"error":"private-provider-secret"}`, status), nil
			})})
			_, err := c.Draft(context.Background(), "draft")
			if !errors.Is(err, ErrUnavailable) || strings.Contains(err.Error(), "private") || calls != 1 {
				t.Fatalf("unsafe failure %v", err)
			}
		})
	}
	for _, body := range []string{"<html>private</html>", strings.Repeat("x", responseLimit+1), `{"id":"x","content":"","usage":{}}`, `{"id":"x","content":"ok","usage":{"input_tokens":-1}}`} {
		c, _ := New(fixtureConfig(t), &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) { return response(body, 200), nil })})
		if _, err := c.Draft(context.Background(), "draft"); !errors.Is(err, ErrUnavailable) {
			t.Fatal("invalid response accepted")
		}
	}
	c, _ := New(fixtureConfig(t), &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) { return nil, errors.New("private-host-token") })})
	_, err := c.Draft(context.Background(), "draft")
	if !errors.Is(err, ErrUnavailable) || strings.Contains(err.Error(), "private") {
		t.Fatal("transport leaked")
	}
	c, _ = New(fixtureConfig(t), &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		r := response("{}", 200)
		r.Header.Set("Content-Type", "text/html")
		return r, nil
	})})
	if _, err = c.Draft(context.Background(), "draft"); !errors.Is(err, ErrUnavailable) {
		t.Fatal("HTML accepted")
	}
	c, _ = New(fixtureConfig(t), &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		r := response("{}", 200)
		r.Body = errorReader{}
		return r, nil
	})})
	if _, err = c.Draft(context.Background(), "draft"); !errors.Is(err, ErrUnavailable) {
		t.Fatal("broken body accepted")
	}
}

type errorReader struct{}

func (errorReader) Read([]byte) (int, error) { return 0, io.ErrUnexpectedEOF }
func (errorReader) Close() error             { return nil }

func TestDiscoveryProjectionAndCaptureFailures(t *testing.T) {
	cfg := fixtureConfig(t)
	for _, body := range []string{`{"url":"https://other.example/","links":[],"count":0,"ranked":false}`, `{"url":"https://triodelab.no/","links":[],"count":1,"ranked":false}`, `{"url":"https://triodelab.no/","links":[],"count":0,"ranked":true}`} {
		c, _ := New(cfg, &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) { return response(body, 200), nil })})
		if _, err := c.Discover(context.Background(), "https://triodelab.no/"); !errors.Is(err, ErrUnavailable) {
			t.Fatal("invalid map accepted")
		}
	}
	c, _ := New(cfg, &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) {
		return response(`{"url":"https://triodelab.no/","links":[{"url":"https://triodelab.no/good"},{"url":"https://triodelab.no/good"},{"url":"https://evil.example/"},{"url":"https://triodelab.no/admin"}],"count":4,"ranked":false}`, 200), nil
	})})
	result, err := c.Discover(context.Background(), "https://triodelab.no/")
	if err != nil || len(result.Pages) != 1 {
		t.Fatalf("unsafe map projection %#v %v", result, err)
	}
	for _, body := range []string{`{}`, `{"data":null}`, `{"data":"wrong-type"}`, `{"data":{},"error":{"message":"private"}}`, `{"run_id":"run_test","url":{"requested":"https://triodelab.no/","final_url":"https://triodelab.no/"},"status":200,"fetched_at":"2026-10-22T12:00:00Z"}`, `{"data":{"run_id":"run_test","url":{"requested":"https://triodelab.no/","final_url":"https://localhost/"},"status":200,"fetched_at":"2026-10-22T12:00:00Z"}}`, `{"data":{"run_id":"run_test","url":{"requested":"https://triodelab.no/","final_url":"https://triodelab.no/"},"status":200,"fetched_at":"not-a-date"}}`} {
		c, _ := New(cfg, &http.Client{Transport: transportFunc(func(*http.Request) (*http.Response, error) { return response(body, 200), nil })})
		if _, err := c.Capture(context.Background(), "https://triodelab.no/", false, true); !errors.Is(err, ErrUnavailable) {
			t.Fatal("invalid capture accepted")
		}
	}
}
