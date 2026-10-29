package workspace

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/webvitals"
)

type WebVitalsCollector struct {
	cfg      pulse.Config
	registry *websites.Service
	client   *http.Client
}

func NewWebVitalsCollector(cfg pulse.Config, registry *websites.Service) *WebVitalsCollector {
	return &WebVitalsCollector{cfg: cfg, registry: registry, client: &http.Client{Timeout: 12 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}

// Collect pulls a bounded site-side receipt only through Integration Core. The
// installation/export key is never leased to Marketing, the browser or agents.
func (c *WebVitalsCollector) Collect(ctx context.Context, record websites.Record, environment string) (webvitals.Export, error) {
	fail := func() (webvitals.Export, error) { return webvitals.Export{}, webvitals.ErrUnavailable }
	if c.registry == nil || pulse.ValidateConfig(pulse.Config{IntegrationCoreURL: c.cfg.IntegrationCoreURL}) != nil {
		return fail()
	}
	current, err := c.registry.Resolve(record.Website.ID, record.Website.Site)
	if err != nil || current.Website.Revision != record.Website.Revision {
		return webvitals.Export{}, websites.ErrConflict
	}
	token, err := webVitalsBearer(c.cfg.IntegrationBearerTokenFile, c.registry.OrganizationID())
	if err != nil {
		return fail()
	}
	w := record.Website
	body, _ := json.Marshal(map[string]any{"websiteId": w.ID, "site": w.Site, "websiteRevision": w.Revision, "environment": environment})
	req, err := http.NewRequestWithContext(ctx, "POST", c.cfg.IntegrationCoreURL+"/api/v1/insights/web-vitals", bytes.NewReader(body))
	if err != nil {
		return fail()
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	response, err := c.client.Do(req)
	if err != nil {
		return fail()
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return fail()
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, (2<<20)+1))
	if err != nil || len(raw) > 2<<20 {
		return fail()
	}
	data, err := webvitals.DecodeReceipt(raw)
	if err != nil {
		return fail()
	}
	current, err = c.registry.Resolve(w.ID, w.Site)
	if err != nil || current.Website.Revision != w.Revision {
		return webvitals.Export{}, websites.ErrConflict
	}
	return data, nil
}
func webVitalsBearer(path, org string) (string, error) {
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return "", webvitals.ErrInvalid
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || info.Mode().Perm() != 0600 || info.Size() < 1 || info.Size() > 8192 {
		return "", webvitals.ErrInvalid
	}
	f, err := os.Open(path)
	if err != nil {
		return "", webvitals.ErrInvalid
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(info, actual) {
		return "", webvitals.ErrInvalid
	}
	raw, err := io.ReadAll(io.LimitReader(f, 8193))
	token := strings.TrimSpace(string(raw))
	if err != nil || len(raw) > 8192 || token == "" || strings.ContainsAny(token, " \r\n\t") {
		return "", webvitals.ErrInvalid
	}
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return "", webvitals.ErrInvalid
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return "", webvitals.ErrInvalid
	}
	var claims struct {
		OrganizationID string `json:"org_id"`
	}
	if json.Unmarshal(payload, &claims) != nil || org == "" || claims.OrganizationID != org {
		return "", webvitals.ErrInvalid
	}
	return token, nil
}
