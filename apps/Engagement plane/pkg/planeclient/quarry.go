package planeclient

import (
	"context"
	"encoding/json"
	"net"
	"net/url"
	"path"
	"regexp"
	"strings"
	"time"
	"unicode"
)

type Page struct {
	URL   string `json:"url"`
	Title string `json:"title,omitempty"`
}
type DiscoveryResult struct {
	Pages []Page `json:"pages"`
}
type Artifact struct {
	ID    string `json:"artifactId"`
	Bytes int64  `json:"bytes"`
}
type CaptureResult struct {
	RunID        string    `json:"runId"`
	RequestedURL string    `json:"requestedUrl"`
	FinalURL     string    `json:"finalUrl"`
	Status       int       `json:"status"`
	FetchedAt    string    `json:"fetchedAt"`
	Fingerprint  string    `json:"fingerprint"`
	Markdown     *Artifact `json:"markdown,omitempty"`
}

func publicURL(raw string, sensitive bool) (*url.URL, error) {
	invalid := func() (*url.URL, error) { return nil, ErrInvalidInput }
	if len(raw) > 2048 || strings.TrimSpace(raw) != raw || strings.IndexFunc(raw, unicode.IsSpace) >= 0 || strings.Contains(raw, "#") {
		return invalid()
	}
	decoded := raw
	for i := 0; i < 4; i++ {
		if strings.Contains(decoded, "\\") || strings.IndexFunc(decoded, unicode.IsControl) >= 0 {
			return invalid()
		}
		next, err := url.PathUnescape(decoded)
		if err != nil {
			return invalid()
		}
		if next == decoded {
			break
		}
		if i == 3 {
			return invalid()
		}
		decoded = next
	}
	u, err := url.Parse(raw)
	if err != nil || u.Scheme != "https" || u.Host == "" || u.User != nil || u.Opaque != "" || u.Port() != "" || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" {
		return invalid()
	}
	host := strings.ToLower(u.Hostname())
	parts := strings.Split(host, ".")
	if net.ParseIP(host) != nil || len(parts) < 2 {
		return invalid()
	}
	for _, label := range parts {
		if !regexp.MustCompile(`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`).MatchString(label) {
			return invalid()
		}
	}
	last := parts[len(parts)-1]
	if !regexp.MustCompile(`[a-z]`).MatchString(last) {
		return invalid()
	}
	for _, denied := range []string{"localhost", "local", "internal", "invalid", "test", "onion"} {
		if last == denied {
			return invalid()
		}
	}
	inspection, err := url.Parse(decoded)
	if err != nil || inspection.Scheme != u.Scheme || inspection.Host != u.Host || inspection.RawQuery != "" || inspection.ForceQuery || inspection.Fragment != "" {
		return invalid()
	}
	clean := path.Clean(inspection.Path)
	if inspection.Path != "" && inspection.Path != "/" && clean != strings.TrimSuffix(inspection.Path, "/") {
		return invalid()
	}
	if sensitive {
		for _, segment := range strings.Split(strings.ToLower(inspection.Path), "/") {
			for _, denied := range []string{"admin", "api", "oauth", "auth", "login", "signin", "sign-in", "callback", "account", "dashboard", "wp-admin", "wp-login.php", "checkout", "reset-password", "unsubscribe"} {
				if segment == denied {
					return invalid()
				}
			}
		}
	}
	return u, nil
}

// Discover calls Quarry's bounded map contract. It does not claim page audits or
// complete site coverage and never fetches a target website directly.
func (c *Client) Discover(ctx context.Context, raw string) (DiscoveryResult, error) {
	empty := DiscoveryResult{Pages: []Page{}}
	u, err := publicURL(raw, true)
	if err != nil {
		return empty, err
	}
	if c == nil || c.cfg.QuarryURL == "" {
		return empty, unavailable("quarry", "not_configured")
	}
	bearer, err := c.bearer(c.cfg.QuarryTokenFile, "quarry", "map:read")
	if err != nil {
		return empty, err
	}
	var result struct {
		URL    string `json:"url"`
		Links  []Page `json:"links"`
		Count  *int   `json:"count"`
		Ranked *bool  `json:"ranked"`
	}
	command := struct {
		URL        string `json:"url"`
		Limit      int    `json:"limit"`
		Subdomains bool   `json:"include_subdomains"`
	}{raw, 25, false}
	if err = c.post(ctx, "quarry", c.cfg.QuarryURL, "/v1/map", bearer, "", command, &result); err != nil {
		return empty, err
	}
	if result.URL != raw || result.Links == nil || len(result.Links) > 25 || result.Count == nil || *result.Count != len(result.Links) || result.Ranked == nil || *result.Ranked {
		return empty, unavailable("quarry", "invalid_source")
	}
	pages := make([]Page, 0, len(result.Links))
	seen := make(map[string]bool)
	for _, page := range result.Links {
		p, e := publicURL(page.URL, true)
		if e != nil || p.Host != u.Host || seen[page.URL] || len(page.Title) > 200 || strings.IndexFunc(page.Title, unicode.IsControl) >= 0 {
			continue
		}
		seen[page.URL] = true
		pages = append(pages, page)
	}
	return DiscoveryResult{Pages: pages}, nil
}

// Capture returns owner-plane metadata and an artifact reference, not raw HTML.
// ingest=true is a write-capable operation; callers must govern its approval.
// ZDR and ingestion cannot be combined. Use ingest=false,zdr=true for read-only.
// A capture response is never proof of durable ingestion: Quarry may have no
// Data Plane transport, or its asynchronous ingestion may fail independently.
func (c *Client) Capture(ctx context.Context, raw string, ingest, zdr bool) (CaptureResult, error) {
	var empty CaptureResult
	if _, err := publicURL(raw, false); err != nil {
		return empty, err
	}
	if ingest && zdr {
		return empty, ErrInvalidInput
	}
	if c == nil || c.cfg.QuarryURL == "" {
		return empty, unavailable("quarry", "not_configured")
	}
	scope := "scrape:read"
	if ingest {
		scope = "scrape:write"
	}
	bearer, err := c.bearer(c.cfg.QuarryTokenFile, "quarry", scope)
	if err != nil {
		return empty, err
	}
	type captureOutput struct {
		RunID string `json:"run_id"`
		URL   struct {
			Requested string `json:"requested"`
			Final     string `json:"final_url"`
		} `json:"url"`
		Status      int    `json:"status"`
		FetchedAt   string `json:"fetched_at"`
		Fingerprint string `json:"fingerprint"`
		Formats     struct {
			Markdown *struct {
				ID    string `json:"artifact_id"`
				Bytes int64  `json:"bytes"`
			} `json:"markdown"`
		} `json:"formats"`
	}
	var envelope struct {
		Data  *captureOutput  `json:"data"`
		Error json.RawMessage `json:"error"`
	}
	command := struct {
		URL    string `json:"url"`
		ZDR    bool   `json:"zdr"`
		Ingest bool   `json:"ingest"`
	}{raw, zdr, ingest}
	if err = c.post(ctx, "quarry", c.cfg.QuarryURL, "/v1/scrape", bearer, "", command, &envelope); err != nil {
		return empty, err
	}
	if envelope.Data == nil || len(envelope.Error) > 0 && string(envelope.Error) != "null" {
		return empty, unavailable("quarry", "invalid_source")
	}
	result := envelope.Data
	if !regexp.MustCompile(`^run_[A-Za-z0-9_-]{1,128}$`).MatchString(result.RunID) || result.URL.Requested != raw || result.Status < 100 || result.Status > 599 || len(result.Fingerprint) > 256 || strings.IndexFunc(result.Fingerprint, unicode.IsControl) >= 0 {
		return empty, unavailable("quarry", "invalid_source")
	}
	if _, err = publicURL(result.URL.Final, false); err != nil {
		return empty, unavailable("quarry", "invalid_source")
	}
	if _, err = time.Parse(time.RFC3339Nano, result.FetchedAt); err != nil {
		return empty, unavailable("quarry", "invalid_source")
	}
	out := CaptureResult{RunID: result.RunID, RequestedURL: raw, FinalURL: result.URL.Final, Status: result.Status, FetchedAt: result.FetchedAt, Fingerprint: result.Fingerprint}
	if ref := result.Formats.Markdown; ref != nil {
		if !regexp.MustCompile(`^art_[A-Za-z0-9_-]{1,128}$`).MatchString(ref.ID) || ref.Bytes < 0 {
			return empty, unavailable("quarry", "invalid_source")
		}
		out.Markdown = &Artifact{ID: ref.ID, Bytes: ref.Bytes}
	}
	return out, nil
}
