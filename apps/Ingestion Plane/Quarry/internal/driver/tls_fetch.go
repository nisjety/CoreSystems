package driver

import (
	"context"
	"crypto/tls"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"golang.org/x/net/html"
)

// TLSFetchDriver is a lightweight HTTP driver that mimics modern browser TLS
// and header fingerprints. It sits between Colly (minimal) and Rod (full
// browser) in the waterfall — faster than launching Chrome but harder to
// fingerprint than a bare Go HTTP client.
//
// Key features over CollyDriver:
//   - Browser-grade TLS cipher suite ordering
//   - Realistic SEC-CH-UA / Accept / Accept-Language headers
//   - HTTP/2 by default (matching Chrome behaviour)
//   - Connection pooling and keep-alive
//   - Proxy support
type TLSFetchDriver struct {
	userAgent   string
	client      *http.Client
	mu          sync.RWMutex
	last        *FetchResult
	fingerprint *BrowserFingerprint // nil → generate per-request
}

// NewTLSFetchDriver creates a new TLS-fingerprint-aware HTTP driver.
func NewTLSFetchDriver(userAgent string) *TLSFetchDriver {
	transport := &http.Transport{
		TLSClientConfig: &tls.Config{
			MinVersion: tls.VersionTLS12,
			CipherSuites: []uint16{
				tls.TLS_AES_128_GCM_SHA256,
				tls.TLS_AES_256_GCM_SHA384,
				tls.TLS_CHACHA20_POLY1305_SHA256,
				tls.TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
				tls.TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
				tls.TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
				tls.TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
				tls.TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256,
				tls.TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256,
			},
		},
		ForceAttemptHTTP2:     true,
		MaxIdleConns:          100,
		MaxIdleConnsPerHost:   10,
		IdleConnTimeout:       90 * time.Second,
		ResponseHeaderTimeout: 30 * time.Second,
		DisableCompression:    false,
	}

	return &TLSFetchDriver{
		userAgent: userAgent,
		client: &http.Client{
			Transport: transport,
			Timeout:   30 * time.Second,
			CheckRedirect: func(req *http.Request, via []*http.Request) error {
				if len(via) >= 10 {
					return fmt.Errorf("too many redirects")
				}
				return nil
			},
		},
	}
}

func (d *TLSFetchDriver) Name() string { return "tls-fetch" }

func (d *TLSFetchDriver) Fetch(ctx context.Context, targetURL string, opts *FetchOptions) (*FetchResult, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, targetURL, nil)
	if err != nil {
		return nil, fmt.Errorf("build request: %w", err)
	}

	// Apply browser-like headers from fingerprint for realistic TLS+HTTP identity.
	fp := d.fingerprint
	if fp == nil {
		tmp := GenerateFingerprint()
		fp = &tmp
	}
	for k, v := range fp.HTTPHeaders() {
		req.Header.Set(k, v)
	}
	// Override UA if explicitly provided.
	if d.userAgent != "" {
		req.Header.Set("User-Agent", d.userAgent)
	}

	// Override with caller headers (e.g. cookies, auth).
	if opts != nil {
		for k, v := range opts.Headers {
			req.Header.Set(k, v)
		}
	}

	// Proxy injection.
	if opts != nil && opts.ProxyURL != "" {
		proxyURL, parseErr := url.Parse(opts.ProxyURL)
		if parseErr == nil {
			transport := d.client.Transport.(*http.Transport).Clone()
			transport.Proxy = http.ProxyURL(proxyURL)
			d.client.Transport = transport
		}
	}

	resp, err := d.client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("tls-fetch request failed: %w", err)
	}
	defer resp.Body.Close()

	body, err := io.ReadAll(io.LimitReader(resp.Body, 25*1024*1024)) // 25 MB limit
	if err != nil {
		return nil, fmt.Errorf("read response body: %w", err)
	}

	htmlStr := string(body)
	links := extractLinksFromHTMLString(htmlStr, targetURL)

	result := &FetchResult{
		URL:         resp.Request.URL.String(), // follow redirect
		Status:      resp.StatusCode,
		ContentType: resp.Header.Get("Content-Type"),
		HTML:        htmlStr,
		RawHTML:     htmlStr,
		Links:       links,
		Rendered:    false,
	}

	d.mu.Lock()
	d.last = result
	d.mu.Unlock()

	return result, nil
}

func (d *TLSFetchDriver) HTML(_ context.Context) (string, error) {
	d.mu.RLock()
	defer d.mu.RUnlock()
	if d.last == nil {
		return "", fmt.Errorf("no page loaded")
	}
	return d.last.HTML, nil
}

// Browser-interactive methods are not supported by the HTTP-only driver.
func (d *TLSFetchDriver) Click(_ context.Context, _ string) error {
	return fmt.Errorf("tls-fetch driver does not support click")
}
func (d *TLSFetchDriver) Type(_ context.Context, _, _ string) error {
	return fmt.Errorf("tls-fetch driver does not support type")
}
func (d *TLSFetchDriver) Press(_ context.Context, _ string) error {
	return fmt.Errorf("tls-fetch driver does not support press")
}
func (d *TLSFetchDriver) Wait(_ context.Context, _ int) error {
	return fmt.Errorf("tls-fetch driver does not support wait")
}
func (d *TLSFetchDriver) Scroll(_ context.Context, _ string) error {
	return fmt.Errorf("tls-fetch driver does not support scroll")
}
func (d *TLSFetchDriver) Screenshot(_ context.Context, _ bool) ([]byte, error) {
	return nil, fmt.Errorf("tls-fetch driver does not support screenshot")
}
func (d *TLSFetchDriver) EvalJS(_ context.Context, _ string) (interface{}, error) {
	return nil, fmt.Errorf("tls-fetch driver does not support EvalJS")
}
func (d *TLSFetchDriver) GeneratePDF(_ context.Context) ([]byte, error) {
	return nil, fmt.Errorf("tls-fetch driver does not support PDF generation")
}
func (d *TLSFetchDriver) Close() error { return nil }

// extractLinksFromHTMLString parses <a href=""> links from raw HTML.
func extractLinksFromHTMLString(rawHTML, baseURL string) []string {
	tokenizer := html.NewTokenizer(strings.NewReader(rawHTML))
	base, _ := url.Parse(baseURL)
	seen := make(map[string]struct{})
	var links []string

	for {
		tt := tokenizer.Next()
		if tt == html.ErrorToken {
			break
		}
		if tt != html.StartTagToken && tt != html.SelfClosingTagToken {
			continue
		}
		t := tokenizer.Token()
		if t.Data != "a" {
			continue
		}
		for _, attr := range t.Attr {
			if attr.Key != "href" {
				continue
			}
			href := strings.TrimSpace(attr.Val)
			if href == "" || strings.HasPrefix(href, "#") || strings.HasPrefix(href, "javascript:") {
				continue
			}
			resolved, err := base.Parse(href)
			if err != nil {
				continue
			}
			abs := resolved.String()
			if _, exists := seen[abs]; !exists {
				seen[abs] = struct{}{}
				links = append(links, abs)
			}
		}
	}
	return links
}
