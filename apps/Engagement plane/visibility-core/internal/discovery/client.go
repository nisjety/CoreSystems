package discovery

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type ClientConfig struct{ URL, TokenFile, OrganizationID string }
type Client struct {
	cfg  ClientConfig
	http *http.Client
}

// NewClient is optional. No fallback to Integration Core's differently scoped
// bearer is permitted. A deployment must harden/authorize Quarry discovery before
// setting these options; the Studio deployment deliberately leaves them unset.
func NewClient(cfg ClientConfig, client *http.Client) (*Client, error) {
	if cfg.URL == "" && cfg.TokenFile == "" {
		return nil, nil
	}
	u, err := url.Parse(cfg.URL)
	if err != nil || u.Host == "" || u.User != nil || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || u.Opaque != "" || (u.Path != "" && u.Path != "/") {
		return nil, errors.New("Quarry discovery requires a fixed origin")
	}
	local := u.Host == "quarry-edge:8082" || u.Host == "127.0.0.1:8082" || u.Host == "localhost:8082" || u.Host == "127.0.0.1:8080" || u.Host == "localhost:8080"
	if u.Scheme != "https" && !(u.Scheme == "http" && local) {
		return nil, errors.New("Quarry requires HTTPS or an approved local origin")
	}
	if cfg.OrganizationID == "" || !filepath.IsAbs(cfg.TokenFile) || filepath.Clean(cfg.TokenFile) != cfg.TokenFile {
		return nil, errors.New("Quarry requires a fixed organization and absolute bearer file")
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	safe := http.Client{Transport: transport}
	if client != nil {
		safe = *client
	}
	safe.Timeout = 30 * time.Second
	safe.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &Client{cfg: cfg, http: &safe}, nil
}

// bearer is a deployment-file binding assertion, NOT signature verification.
// Quarry independently verifies RS256, audience, org and route authorization.
func (c *Client) bearer() (string, error) {
	f, err := os.Open(c.cfg.TokenFile)
	if err != nil {
		return "", &CaptureError{Reason: "access_denied"}
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 {
		return "", &CaptureError{Reason: "access_denied"}
	}
	raw, err := io.ReadAll(io.LimitReader(f, 8193))
	if err != nil || len(raw) > 8192 {
		return "", &CaptureError{Reason: "access_denied"}
	}
	token := strings.TrimSpace(string(raw))
	parts := strings.Split(token, ".")
	if len(parts) != 3 || strings.ContainsAny(token, " \r\n\t") {
		return "", &CaptureError{Reason: "access_denied"}
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return "", &CaptureError{Reason: "access_denied"}
	}
	var claims struct {
		Org      string `json:"org_id"`
		Audience string `json:"aud"`
		Expires  int64  `json:"exp"`
	}
	if json.Unmarshal(payload, &claims) != nil || claims.Org != c.cfg.OrganizationID || claims.Audience != "quarry" || claims.Expires <= time.Now().Add(15*time.Second).Unix() {
		return "", &CaptureError{Reason: "access_denied"}
	}
	return token, nil
}
func (c *Client) Map(ctx context.Context, site string) ([]Page, error) {
	pages, _, err := c.MapWithCoverage(ctx, site)
	return pages, err
}
func (c *Client) MapWithCoverage(ctx context.Context, site string) ([]Page, *MapCoverage, error) {
	origin, err := pulse.CanonicalSite(site)
	if err != nil || origin != site {
		return nil, nil, &CaptureError{Reason: "invalid_source"}
	}
	token, err := c.bearer()
	if err != nil {
		return nil, nil, err
	}
	body, _ := json.Marshal(struct {
		URL        string `json:"url"`
		Limit      int    `json:"limit"`
		Subdomains bool   `json:"include_subdomains"`
	}{site, Limit, false})
	r, err := http.NewRequestWithContext(ctx, "POST", strings.TrimRight(c.cfg.URL, "/")+"/v1/map", bytes.NewReader(body))
	if err != nil {
		return nil, nil, &CaptureError{Reason: "source_unavailable"}
	}
	r.Header.Set("Content-Type", "application/json")
	r.Header.Set("Accept", "application/json")
	r.Header.Set("Authorization", "Bearer "+token)
	response, err := c.http.Do(r)
	if err != nil {
		return nil, nil, &CaptureError{Reason: "source_unavailable"}
	}
	defer response.Body.Close()
	switch response.StatusCode {
	case 200:
	case 401:
		return nil, nil, &CaptureError{Reason: "access_denied"}
	case 403:
		// Route authorization returns an empty 403. A bounded, structured
		// transport denial is distinct: asking users to reconnect cannot fix it.
		var blocked struct {
			Code string `json:"code"`
		}
		if strings.HasPrefix(response.Header.Get("Content-Type"), "application/json") {
			raw, readErr := io.ReadAll(io.LimitReader(response.Body, 4097))
			if readErr == nil && len(raw) <= 4096 && json.Unmarshal(raw, &blocked) == nil && (blocked.Code == "FORBIDDEN" || blocked.Code == "SECURITY_BLOCKED") {
				return nil, nil, &CaptureError{Reason: "policy_blocked"}
			}
		}
		return nil, nil, &CaptureError{Reason: "access_denied"}
	case 429:
		return nil, nil, &CaptureError{Reason: "quota_limited"}
	default:
		return nil, nil, &CaptureError{Reason: "source_unavailable"}
	}
	if !strings.HasPrefix(response.Header.Get("Content-Type"), "application/json") {
		return nil, nil, &CaptureError{Reason: "invalid_source"}
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, 262145))
	if err != nil || len(raw) > 262144 {
		return nil, nil, &CaptureError{Reason: "invalid_source"}
	}
	var result struct {
		URL      string `json:"url"`
		Links    []Page `json:"links"`
		Count    *int   `json:"count"`
		Ranked   *bool  `json:"ranked"`
		Coverage *struct {
			SitemapFiles   int  `json:"sitemap_files"`
			SitemapValid   int  `json:"sitemap_valid"`
			SitemapPartial bool `json:"sitemap_partial"`
			Candidates     int  `json:"candidates"`
			RobotsFiltered int  `json:"robots_filtered"`
			Truncated      bool `json:"truncated"`
		} `json:"coverage"`
	}
	if json.Unmarshal(raw, &result) != nil || result.URL != site || result.Links == nil || len(result.Links) > Limit || result.Count == nil || *result.Count != len(result.Links) || result.Ranked == nil || *result.Ranked {
		return nil, nil, &CaptureError{Reason: "invalid_source"}
	}
	var coverage *MapCoverage
	if v := result.Coverage; v != nil {
		if v.SitemapFiles < 0 || v.SitemapFiles > 20 || v.SitemapValid < 0 || v.SitemapValid > v.SitemapFiles || v.Candidates < 0 || v.Candidates > 2000 || v.RobotsFiltered < 0 || v.RobotsFiltered > 2000 {
			return nil, nil, &CaptureError{Reason: "invalid_source"}
		}
		coverage = &MapCoverage{v.SitemapFiles, v.SitemapValid, v.SitemapPartial, v.Candidates, v.RobotsFiltered, v.Truncated}
	}
	return sanitizePages(site, result.Links), coverage, nil
}
