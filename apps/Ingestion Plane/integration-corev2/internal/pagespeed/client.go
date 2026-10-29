// Package pagespeed owns fixed-endpoint, read-only PageSpeed diagnostics.
// It does not lease OAuth credentials or retain the raw Lighthouse response.
package pagespeed

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"math"
	"net"
	"net/http"
	"net/url"
	"strings"
	"time"

	"github.com/triodelab/integration-corev2/internal/egress"
)

const providerEndpoint = "https://www.googleapis.com/pagespeedonline/v5/runPagespeed"
const maxResponseBytes = 5 << 20

// Report contains Lighthouse lab measurements only. TBT is not field INP.
type Report struct {
	ReportingOrigin   string   `json:"reportingOrigin,omitempty"`
	Performance       *float64 `json:"performance"`
	Accessibility     *float64 `json:"accessibility"`
	BestPractices     *float64 `json:"bestPractices"`
	SEO               *float64 `json:"seo"`
	LCPMs             *float64 `json:"lcpMs"`
	CLS               *float64 `json:"cls"`
	TBTMs             *float64 `json:"tbtMs"`
	FCPMs             *float64 `json:"fcpMs"`
	SpeedIndexMs      *float64 `json:"speedIndexMs"`
	FetchTime         string   `json:"fetchTime"`
	LighthouseVersion string   `json:"lighthouseVersion"`
	Checks            []Check  `json:"checks,omitempty"`
}

type Client struct {
	apiKey string
	http   *http.Client
}

func NewClient(apiKey string, client *http.Client) *Client {
	if client == nil {
		client = egress.SafeClient(egress.ClientConfig{RequestTimeout: 75 * time.Second})
	}
	bounded := *client
	bounded.Timeout = 75 * time.Second
	bounded.CheckRedirect = func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }
	return &Client{apiKey: apiKey, http: &bounded}
}

// ValidateInput accepts only already-canonical, public-looking HTTPS origins.
// The origin is passed to Google, never fetched by this service. The only
// outbound destination is providerEndpoint, guarded at dial time by egress.
func ValidateInput(site, strategy string) error {
	if strategy != "mobile" && strategy != "desktop" {
		return errors.New("strategy must be mobile or desktop")
	}
	u, err := url.Parse(site)
	if err != nil || len(site) > 270 || u.Scheme != "https" || u.User != nil || u.Port() != "" || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || u.RawFragment != "" || u.Path != "/" || u.RawPath != "" || site != "https://"+u.Host+"/" {
		return errors.New("site must be a canonical public HTTPS origin")
	}
	host := u.Hostname()
	if host != u.Host || host != strings.ToLower(host) || len(host) > 253 || net.ParseIP(host) != nil {
		return errors.New("site hostname is invalid")
	}
	labels := strings.Split(host, ".")
	if len(labels) < 2 {
		return errors.New("site requires a public hostname")
	}
	for _, label := range labels {
		if len(label) < 1 || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return errors.New("site hostname is invalid")
		}
		for _, char := range label {
			if !(char >= 'a' && char <= 'z' || char >= '0' && char <= '9' || char == '-') {
				return errors.New("site hostname is invalid")
			}
		}
	}
	tld := labels[len(labels)-1]
	if len(tld) < 2 || strings.Trim(tld, "0123456789") == "" {
		return errors.New("site hostname is invalid")
	}
	for _, suffix := range []string{"localhost", "local", "internal", "invalid", "test", "onion", "home", "lan"} {
		if tld == suffix {
			return errors.New("site must be public")
		}
	}
	return nil
}

func (c *Client) Run(ctx context.Context, site, strategy string) (*Report, string) {
	if ValidateInput(site, strategy) != nil {
		return nil, "audit_failed"
	}
	u, _ := url.Parse(providerEndpoint)
	query := url.Values{"url": {site}, "strategy": {strategy}, "category": {"performance", "accessibility", "best-practices", "seo"}}
	if c.apiKey != "" {
		query.Set("key", c.apiKey)
	}
	u.RawQuery = query.Encode()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, u.String(), nil)
	if err != nil {
		return nil, "source_unavailable"
	}
	request.Header.Set("Accept", "application/json")
	response, err := c.http.Do(request)
	// Transport errors may contain the secret-bearing request URL. They must
	// never be returned, logged, or persisted by this boundary.
	if err != nil {
		return nil, "source_unavailable"
	}
	defer response.Body.Close()
	if response.StatusCode == http.StatusTooManyRequests {
		return nil, "quota_limited"
	}
	if response.StatusCode == http.StatusForbidden || response.StatusCode == http.StatusUnauthorized {
		return nil, "not_configured"
	}
	if response.StatusCode != http.StatusOK {
		return nil, "source_unavailable"
	}
	if response.ContentLength > maxResponseBytes {
		return nil, "audit_failed"
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes+1))
	if err != nil || len(raw) > maxResponseBytes {
		return nil, "audit_failed"
	}
	return decodeReport(raw, site, strategy)
}

type lighthouseResult struct {
	RequestedURL   string `json:"requestedUrl"`
	FinalURL       string `json:"finalUrl"`
	ConfigSettings struct {
		FormFactor         string `json:"formFactor"`
		EmulatedFormFactor string `json:"emulatedFormFactor"`
	} `json:"configSettings"`
	FetchTime         string `json:"fetchTime"`
	LighthouseVersion string `json:"lighthouseVersion"`
	Categories        map[string]struct {
		Score *float64 `json:"score"`
	} `json:"categories"`
	Audits       map[string]json.RawMessage `json:"audits"`
	RuntimeError *struct {
		Code string `json:"code"`
	} `json:"runtimeError"`
}

func decodeReport(raw []byte, site, strategy string) (*Report, string) {
	var response struct {
		Result *lighthouseResult `json:"lighthouseResult"`
	}
	if json.Unmarshal(raw, &response) != nil || response.Result == nil {
		return nil, "audit_failed"
	}
	r := response.Result
	if r.RequestedURL != site {
		return nil, "audit_failed"
	}
	actualOrigin := ""
	if r.FinalURL != "" {
		actual, err := url.Parse(r.FinalURL)
		requested, _ := url.Parse(site)
		root := strings.TrimPrefix(requested.Hostname(), "www.")
		if err != nil || actual.Scheme != "https" || actual.User != nil || actual.Port() != "" || (actual.Host != root && actual.Host != "www."+root) {
			return nil, "audit_failed"
		}
		actualOrigin = "https://" + actual.Host
	}
	factor := r.ConfigSettings.FormFactor
	if factor == "" {
		factor = r.ConfigSettings.EmulatedFormFactor
	}
	if factor != strategy || r.ConfigSettings.EmulatedFormFactor != "" && r.ConfigSettings.EmulatedFormFactor != strategy {
		return nil, "audit_failed"
	}
	if r.RuntimeError != nil || len(r.Categories) == 0 || len(r.LighthouseVersion) > 32 || r.LighthouseVersion == "" {
		return nil, "audit_failed"
	}
	if _, err := time.Parse(time.RFC3339Nano, r.FetchTime); err != nil {
		return nil, "audit_failed"
	}
	for _, char := range r.LighthouseVersion {
		if !(char >= 'a' && char <= 'z' || char >= 'A' && char <= 'Z' || char >= '0' && char <= '9' || char == '.' || char == '-' || char == '+') {
			return nil, "audit_failed"
		}
	}
	for _, name := range []string{"performance", "accessibility", "best-practices", "seo"} {
		category := r.Categories[name]
		if category.Score != nil && !finiteBetween(*category.Score, 0, 1) {
			return nil, "audit_failed"
		}
	}
	metrics := make(map[string]*float64, 5)
	for _, name := range []string{"largest-contentful-paint", "cumulative-layout-shift", "total-blocking-time", "first-contentful-paint", "speed-index"} {
		value, valid := numericAudit(r.Audits[name])
		if !valid {
			return nil, "audit_failed"
		}
		metrics[name] = value
	}
	score := func(name string) *float64 {
		if v := r.Categories[name].Score; v != nil {
			scaled := *v * 100
			return &scaled
		}
		return nil
	}
	metric := func(name string) *float64 {
		if v := metrics[name]; v != nil {
			copied := *v
			return &copied
		}
		return nil
	}
	return &Report{
		ReportingOrigin: actualOrigin,
		Performance:     score("performance"), Accessibility: score("accessibility"), BestPractices: score("best-practices"), SEO: score("seo"),
		LCPMs: metric("largest-contentful-paint"), CLS: metric("cumulative-layout-shift"), TBTMs: metric("total-blocking-time"), FCPMs: metric("first-contentful-paint"), SpeedIndexMs: metric("speed-index"), FetchTime: r.FetchTime, LighthouseVersion: r.LighthouseVersion,
		Checks: projectChecks(r.Audits),
	}, ""
}

func finiteBetween(value, low, high float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0) && value >= low && value <= high
}
