package siteaudit

import (
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

type roundTrip func(*http.Request) (*http.Response, error)

func (f roundTrip) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func clientToken(t *testing.T, org, aud string) string {
	t.Helper()
	raw, _ := json.Marshal(map[string]any{"org_id": org, "aud": aud, "exp": time.Now().Add(time.Hour).Unix()})
	file := filepath.Join(t.TempDir(), "bearer")
	if err := os.WriteFile(file, []byte("header."+base64.RawURLEncoding.EncodeToString(raw)+".signature"), 0600); err != nil {
		t.Fatal(err)
	}
	return file
}
func captureBody() string {
	v := map[string]any{"version": "bounded_page_v1", "url": testSite, "final_url": testSite, "status": 200, "fetched_at": "2026-10-22T10:00:00Z", "run_id": "run_01K7K6MYJ0B6E321J1K5S8M2J7", "fingerprint": "blake3:" + strings.Repeat("a", 64), "metadata": map[string]any{"title": "Title", "description": "Description", "content_type": "text/html"}, "canonical": testSite, "links": []map[string]string{{"url": testSite + "innsikt"}}, "partial": true, "limits": map[string]int{"body_bytes": 1048576, "timeout_ms": 10000, "max_links": 100}}
	raw, _ := json.Marshal(v)
	return string(raw)
}
func TestClientUsesBoundedQuarryContractAndFixedCredential(t *testing.T) {
	calls := 0
	cfg := discovery.ClientConfig{URL: "http://quarry-edge:8082", TokenFile: clientToken(t, "org-one", "quarry"), OrganizationID: "org-one"}
	c, err := NewClient(cfg, &http.Client{Transport: roundTrip(func(r *http.Request) (*http.Response, error) {
		calls++
		if r.URL.String() != "http://quarry-edge:8082/v1/audit/page" || r.Method != "POST" || !strings.HasPrefix(r.Header.Get("Authorization"), "Bearer ") {
			t.Fatal("unbounded endpoint")
		}
		raw, _ := io.ReadAll(r.Body)
		if string(raw) != `{"url":"https://triodelab.no/"}` {
			t.Fatalf("unexpected provider request %s", raw)
		}
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(captureBody()))}, nil
	})})
	if err != nil {
		t.Fatal(err)
	}
	p, err := c.Capture(t.Context(), testSite, testSite)
	if err != nil || p.Title != "Title" || len(p.Links) != 1 {
		t.Fatal(p, err)
	}
	if _, err = c.Capture(t.Context(), testSite, "https://evil.test/"); err == nil {
		t.Fatal("crosssite fetched")
	}
	c.cfg.TokenFile = clientToken(t, "other", "quarry")
	if _, err = c.Capture(t.Context(), testSite, testSite); err == nil {
		t.Fatal("foreign bearer sent")
	}
	if calls != 1 {
		t.Fatal("unexpected calls", calls)
	}
}
func TestClientRejectsUnboundedOrMalformedContracts(t *testing.T) {
	for _, tc := range []struct {
		status int
		body   string
	}{{200, strings.Replace(captureBody(), "bounded_page_v1", "generic_scrape", 1)}, {200, strings.Replace(captureBody(), "1048576", "9000000", 1)}, {200, strings.Replace(captureBody(), `"partial":true`, `"partial":null`, 1)}, {200, `{"data":{}}`}, {200, strings.Repeat("a", 512*1024+1)}, {401, "secret"}, {403, `{"code":"FORBIDDEN"}`}, {429, "secret"}, {302, "secret"}, {500, "secret"}} {
		t.Run(tc.body[:min(24, len(tc.body))], func(t *testing.T) {
			c, err := NewClient(discovery.ClientConfig{URL: "http://quarry-edge:8082", TokenFile: clientToken(t, "org-one", "quarry"), OrganizationID: "org-one"}, &http.Client{Transport: roundTrip(func(*http.Request) (*http.Response, error) {
				return &http.Response{StatusCode: tc.status, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(tc.body))}, nil
			})})
			if err != nil {
				t.Fatal(err)
			}
			if _, err = c.Capture(t.Context(), testSite, testSite); err == nil {
				t.Fatal("invalid contract accepted")
			}
		})
	}
}
func TestClientCredentialFilesFailClosed(t *testing.T) {
	cfg := discovery.ClientConfig{URL: "https://quarry.test", TokenFile: clientToken(t, "org-one", "quarry"), OrganizationID: "org-one"}
	c, _ := NewClient(cfg, nil)
	for _, value := range []string{"a.b.c", "bad", strings.Repeat("x", 8193)} {
		if err := os.WriteFile(cfg.TokenFile, []byte(value), 0600); err != nil {
			t.Fatal(err)
		}
		if _, err := c.token(); err == nil {
			t.Fatal("bad token accepted")
		}
	}
	file := clientToken(t, "org-one", "quarry")
	c.cfg.TokenFile = file
	if err := os.Chmod(file, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := c.token(); err == nil {
		t.Fatal("public secret")
	}
	if err := os.Chmod(file, 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(t.TempDir(), "link")
	_ = os.Symlink(file, link)
	c.cfg.TokenFile = link
	if _, err := c.token(); err == nil {
		t.Fatal("secret symlink")
	}
	if _, err := NewClient(discovery.ClientConfig{}, nil); err != nil {
		t.Fatal(err)
	}
	if _, err := NewClient(discovery.ClientConfig{URL: "http://evil.test", TokenFile: file, OrganizationID: "org-one"}, nil); err == nil {
		t.Fatal("unsafe origin")
	}
}
