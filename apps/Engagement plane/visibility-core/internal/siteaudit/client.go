package siteaudit

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"mime"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
)

type Client struct {
	cfg       discovery.ClientConfig
	inventory *discovery.Client
	http      *http.Client
}

// NewClient deliberately has no generic /scrape fallback. Only Quarry's
// transport-bounded, credentialless page projection is accepted.
func NewClient(cfg discovery.ClientConfig, client *http.Client) (*Client, error) {
	inventory, err := discovery.NewClient(cfg, client)
	if err != nil || inventory == nil {
		return nil, err
	}
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	safe := http.Client{Transport: transport}
	if client != nil {
		safe = *client
	}
	safe.Timeout = 15 * time.Second
	safe.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &Client{cfg: cfg, inventory: inventory, http: &safe}, nil
}
func (c *Client) Map(ctx context.Context, site string) ([]discovery.Page, error) {
	return c.inventory.Map(ctx, site)
}
func (c *Client) MapWithCoverage(ctx context.Context, site string) ([]discovery.Page, *discovery.MapCoverage, error) {
	return c.inventory.MapWithCoverage(ctx, site)
}
func (c *Client) token() (string, error) {
	invalid := func() (string, error) { return "", &CaptureError{Reason: "access_denied"} }
	info, err := os.Lstat(c.cfg.TokenFile)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm()&0077 != 0 {
		return invalid()
	}
	f, err := os.Open(c.cfg.TokenFile)
	if err != nil {
		return invalid()
	}
	defer f.Close()
	current, err := f.Stat()
	if err != nil || !os.SameFile(info, current) {
		return invalid()
	}
	raw, err := io.ReadAll(io.LimitReader(f, 8193))
	if err != nil || len(raw) > 8192 {
		return invalid()
	}
	token := strings.TrimSpace(string(raw))
	parts := strings.Split(token, ".")
	if len(parts) != 3 || strings.ContainsAny(token, " \r\n\t") {
		return invalid()
	}
	decoded, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return invalid()
	}
	var claims struct {
		Org      string `json:"org_id"`
		Audience string `json:"aud"`
		Expires  int64  `json:"exp"`
	}
	if json.Unmarshal(decoded, &claims) != nil || claims.Org != c.cfg.OrganizationID || claims.Audience != "quarry" || claims.Expires <= time.Now().Add(15*time.Second).Unix() {
		return invalid()
	}
	return token, nil
}

type boundedPage struct {
	Version     string `json:"version"`
	URL         string `json:"url"`
	FinalURL    string `json:"final_url"`
	Status      int    `json:"status"`
	FetchedAt   string `json:"fetched_at"`
	RunID       string `json:"run_id"`
	Fingerprint string `json:"fingerprint"`
	Metadata    struct {
		Title       *string `json:"title"`
		Description *string `json:"description"`
		ContentType *string `json:"content_type"`
	} `json:"metadata"`
	Canonical *string `json:"canonical"`
	Links     []struct {
		URL string `json:"url"`
	} `json:"links"`
	Partial *bool `json:"partial"`
	Limits  struct {
		BodyBytes int `json:"body_bytes"`
		TimeoutMS int `json:"timeout_ms"`
		MaxLinks  int `json:"max_links"`
	} `json:"limits"`
	Headings  []Heading  `json:"headings"`
	Redirects []Redirect `json:"redirects"`
	SEO       *struct {
		Version           string         `json:"version"`
		Noindex           bool           `json:"noindex"`
		Nofollow          bool           `json:"nofollow"`
		RobotsStatus      int            `json:"robots_status"`
		WordCount         int            `json:"word_count"`
		Alternates        []Alternate    `json:"hreflang"`
		InvalidAlternates int            `json:"invalid_alternates"`
		AlternatesPartial bool           `json:"alternates_partial"`
		StructuredData    StructuredData `json:"structured_data"`
	} `json:"seo"`
}

func (c *Client) Capture(ctx context.Context, site, raw string) (CapturePage, error) {
	if !discovery.SafeURL(site, raw) {
		return CapturePage{}, &CaptureError{Reason: "invalid_source"}
	}
	token, err := c.token()
	if err != nil {
		return CapturePage{}, err
	}
	body, _ := json.Marshal(struct {
		URL string `json:"url"`
	}{raw})
	r, err := http.NewRequestWithContext(ctx, http.MethodPost, strings.TrimRight(c.cfg.URL, "/")+"/v1/audit/page", bytes.NewReader(body))
	if err != nil {
		return CapturePage{}, &CaptureError{Reason: "source_unavailable"}
	}
	r.Header.Set("Content-Type", "application/json")
	r.Header.Set("Accept", "application/json")
	r.Header.Set("Authorization", "Bearer "+token)
	response, err := c.http.Do(r)
	if err != nil {
		return CapturePage{}, &CaptureError{Reason: reason(err)}
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return CapturePage{}, responseFailure(response.StatusCode)
	}
	media, _, err := mime.ParseMediaType(response.Header.Get("Content-Type"))
	if err != nil || media != "application/json" {
		return CapturePage{}, &CaptureError{Reason: "invalid_source"}
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, 512*1024+1))
	if err != nil || len(data) > 512*1024 {
		return CapturePage{}, &CaptureError{Reason: "invalid_source"}
	}
	var value boundedPage
	if json.Unmarshal(data, &value) != nil || value.Version != "bounded_page_v1" || value.URL != raw || value.Partial == nil || value.Limits.BodyBytes != 1048576 || value.Limits.TimeoutMS != 10000 || value.Limits.MaxLinks != maxLinks || value.Links == nil || len(value.Links) > maxLinks {
		return CapturePage{}, &CaptureError{Reason: "invalid_source"}
	}
	links := []string{}
	for _, link := range value.Links {
		links = append(links, link.URL)
	}
	var seo *SEO
	if value.SEO != nil {
		var fields map[string]json.RawMessage
		if json.Unmarshal(data, &fields) != nil || !requiredEvidence(fields["seo"], "version", "noindex", "nofollow", "robots_status", "word_count", "hreflang", "invalid_alternates", "alternates_partial", "structured_data") || value.Headings == nil || value.Redirects == nil {
			return CapturePage{}, &CaptureError{Reason: "invalid_source"}
		}
		var seoFields map[string]json.RawMessage
		if json.Unmarshal(fields["seo"], &seoFields) != nil || !requiredEvidence(seoFields["structured_data"], "documents", "invalid", "types", "partial") {
			return CapturePage{}, &CaptureError{Reason: "invalid_source"}
		}
		v := value.SEO
		seo = &SEO{Version: v.Version, Headings: append([]Heading{}, value.Headings...), HeadingsPartial: len(value.Headings) >= 200 && *value.Partial, Noindex: v.Noindex, Nofollow: v.Nofollow, RobotsStatus: v.RobotsStatus, WordCount: v.WordCount, Alternates: []Alternate{}, InvalidAlternates: v.InvalidAlternates, AlternatesPartial: v.AlternatesPartial, StructuredData: v.StructuredData, Redirects: append([]Redirect{}, value.Redirects...)}
		for _, a := range v.Alternates {
			if discovery.SafeURL(site, a.URL) {
				seo.Alternates = append(seo.Alternates, a)
			} else {
				seo.AlternatesPartial = true
			}
		}
		if value.Canonical != nil && *value.Canonical != "" && !discovery.SafeURL(site, *value.Canonical) {
			seo.CanonicalOutsideSite = true
		}
		if !validSEO(site, seo) {
			return CapturePage{}, &CaptureError{Reason: "invalid_source"}
		}
	}
	return CapturePage{URL: value.URL, FinalURL: value.FinalURL, HTTPStatus: value.Status, Title: text(value.Metadata.Title), Description: text(value.Metadata.Description), ContentType: text(value.Metadata.ContentType), Canonical: text(value.Canonical), ObservedAt: value.FetchedAt, RunID: value.RunID, Fingerprint: value.Fingerprint, Links: links, SEO: seo, Partial: *value.Partial}, nil
}

func requiredEvidence(raw json.RawMessage, keys ...string) bool {
	var value map[string]json.RawMessage
	if json.Unmarshal(raw, &value) != nil || value == nil {
		return false
	}
	for _, key := range keys {
		field, ok := value[key]
		if !ok || string(bytes.TrimSpace(field)) == "null" {
			return false
		}
	}
	return true
}
func text(value *string) string {
	if value == nil {
		return ""
	}
	return *value
}
func responseFailure(status int) error {
	reason := "source_unavailable"
	switch status {
	case 401:
		reason = "access_denied"
	case 403:
		reason = "policy_blocked"
	case 429:
		reason = "quota_limited"
	case 504, 408:
		reason = "timeout"
	case 400, 413, 415:
		reason = "invalid_source"
	}
	return &CaptureError{Reason: reason}
}
