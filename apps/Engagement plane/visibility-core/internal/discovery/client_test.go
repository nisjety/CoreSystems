package discovery

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type roundTrip func(*http.Request) (*http.Response, error)

func (f roundTrip) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func tokenFile(t *testing.T, org, aud string) string {
	t.Helper()
	claims, _ := json.Marshal(map[string]any{"org_id": org, "aud": aud, "exp": time.Now().Add(time.Hour).Unix()})
	p := filepath.Join(t.TempDir(), "token")
	if err := os.WriteFile(p, []byte("header."+base64.RawURLEncoding.EncodeToString(claims)+".signature"), 0600); err != nil {
		t.Fatal(err)
	}
	return p
}
func TestQuarryClientBindsCredentialAndFixedRequest(t *testing.T) {
	path := tokenFile(t, "org-one", "quarry")
	calls := 0
	transport := roundTrip(func(r *http.Request) (*http.Response, error) {
		calls++
		if r.URL.String() != "http://quarry-edge:8082/v1/map" || r.Method != "POST" || !strings.HasPrefix(r.Header.Get("Authorization"), "Bearer ") {
			t.Fatal("wrong fixed endpoint/credential")
		}
		b, _ := io.ReadAll(r.Body)
		if string(b) != `{"url":"https://triodelab.no/","limit":250,"include_subdomains":false}` {
			t.Fatalf("body %s", b)
		}
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(`{"url":"https://triodelab.no/","links":[{"url":"https://triodelab.no/innsikt","title":"Innsikt","token":"private"}],"count":1,"ranked":false,"token":"secret"}`))}, nil
	})
	cfg := ClientConfig{URL: "http://quarry-edge:8082", TokenFile: path, OrganizationID: "org-one"}
	c, err := NewClient(cfg, &http.Client{Transport: transport})
	if err != nil {
		t.Fatal(err)
	}
	got, err := c.Map(t.Context(), testSite)
	if err != nil || len(got) != 1 || got[0].Title != "Innsikt" {
		t.Fatalf("result %+v %v", got, err)
	}
	for _, p := range []string{tokenFile(t, "other", "quarry"), tokenFile(t, "org-one", "ingestion"), filepath.Join(t.TempDir(), "absent")} {
		c.cfg.TokenFile = p
		if _, err := c.Map(t.Context(), testSite); err == nil {
			t.Fatal("wrong bearer reached Quarry")
		}
	}
	if calls != 1 {
		t.Fatal("credential boundary failed")
	}
}
func TestQuarryClientRejectsProviderFailuresAndUnboundedData(t *testing.T) {
	for _, tc := range []struct {
		status       int
		body, reason string
	}{{401, `secret`, "access_denied"}, {403, `secret`, "access_denied"}, {429, `secret`, "quota_limited"}, {302, `secret`, "source_unavailable"}, {500, `secret`, "source_unavailable"}, {200, `{"url":"https://other.no/","links":[],"count":0,"ranked":false}`, "invalid_source"}, {200, `{"url":"https://triodelab.no/","links":[],"count":1,"ranked":false}`, "invalid_source"}, {200, strings.Repeat("x", 262145), "invalid_source"}, {200, `{"url":"https://triodelab.no/","links":[],"count":0,"ranked":true}`, "invalid_source"}} {
		t.Run(tc.reason+string(rune(tc.status)), func(t *testing.T) {
			c, _ := NewClient(ClientConfig{URL: "http://quarry-edge:8082", TokenFile: tokenFile(t, "org-one", "quarry"), OrganizationID: "org-one"}, &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: tc.status, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(tc.body))}, nil
			})})
			_, err := c.Map(context.Background(), testSite)
			e, ok := err.(*CaptureError)
			if !ok || e.Reason != tc.reason {
				t.Fatalf("%v", err)
			}
		})
	}
}
func TestQuarryClientConfigurationAndSiteFailClosed(t *testing.T) {
	for _, endpoint := range []string{"http://evil.test/", "http://quarry-edge:8082/path", "https://user:pass@quarry.test", "https://quarry.test/?key=secret", "https://quarry.test/#secret", "http://127.0.0.1:9999"} {
		if _, err := NewClient(ClientConfig{URL: endpoint, TokenFile: "/private/token", OrganizationID: "org-one"}, nil); err == nil {
			t.Fatal("bad config", endpoint)
		}
	}
	if _, err := NewClient(ClientConfig{}, nil); err != nil {
		t.Fatal("optional config")
	}
	c, err := NewClient(ClientConfig{URL: "https://quarry.test", TokenFile: "/private/token", OrganizationID: "org-one"}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := c.Map(t.Context(), "http://localhost/"); err == nil {
		t.Fatal("unsafe site")
	}
}
