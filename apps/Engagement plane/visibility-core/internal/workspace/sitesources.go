package workspace

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type SiteSourcesCollector struct {
	cfg      pulse.Config
	registry *websites.Service
	client   *http.Client
}

func NewSiteSourcesCollector(cfg pulse.Config, registry *websites.Service) *SiteSourcesCollector {
	return &SiteSourcesCollector{cfg: cfg, registry: registry, client: &http.Client{Timeout: 21 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}

// Collect leases no provider key. Integration verifies the signed org bearer,
// resolves protected mappings and makes all direct Cookiebot/Algolia requests.
func (c *SiteSourcesCollector) Collect(ctx context.Context, record websites.Record, provider string) (sitesources.Report, error) {
	fail := func() (sitesources.Report, error) { return sitesources.Report{}, sitesources.ErrUnavailable }
	if c.registry == nil || provider != "cookiebot" && provider != "algolia" || pulse.ValidateConfig(pulse.Config{IntegrationCoreURL: c.cfg.IntegrationCoreURL}) != nil {
		return fail()
	}
	if !c.current(record) {
		return sitesources.Report{}, websites.ErrConflict
	}
	// Reuse the existing same-file, private-permission org bearer reader.
	token, err := webVitalsBearer(c.cfg.IntegrationBearerTokenFile, c.registry.OrganizationID())
	if err != nil {
		return fail()
	}
	w := record.Website
	body, _ := json.Marshal(map[string]any{"websiteId": w.ID, "site": w.Site, "websiteRevision": w.Revision, "provider": provider})
	req, err := http.NewRequestWithContext(ctx, "POST", c.cfg.IntegrationCoreURL+"/api/v1/insights/site-sources", bytes.NewReader(body))
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
	raw, err := io.ReadAll(io.LimitReader(response.Body, sitesources.MaxReceiptBytes+1))
	if err != nil || len(raw) > sitesources.MaxReceiptBytes {
		return fail()
	}
	data, err := sitesources.DecodeReceipt(raw)
	if err != nil {
		return fail()
	}
	if !c.current(record) {
		return sitesources.Report{}, websites.ErrConflict
	}
	return data, nil
}
func (c *SiteSourcesCollector) current(record websites.Record) bool {
	w := record.Website
	current, err := c.registry.Resolve(w.ID, w.Site)
	return err == nil && current.Website.Revision == w.Revision
}
