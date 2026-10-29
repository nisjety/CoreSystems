package workspace

import (
	"bytes"
	"context"
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/algoliamapping"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"io"
	"net/http"
	"time"
)

type AlgoliaMappingClient struct {
	cfg      pulse.Config
	registry *websites.Service
	client   *http.Client
}

func NewAlgoliaMappingClient(cfg pulse.Config, registry *websites.Service) *AlgoliaMappingClient {
	return &AlgoliaMappingClient{cfg: cfg, registry: registry, client: &http.Client{Timeout: 32 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}
func (c *AlgoliaMappingClient) post(ctx context.Context, path string, body any) ([]byte, error) {
	budget := 21 * time.Second
	if path == "/api/v1/insights/algolia/report" {
		budget = 32 * time.Second
	}
	ctx, cancel := context.WithTimeout(ctx, budget)
	defer cancel()
	if c.registry == nil || pulse.ValidateConfig(pulse.Config{IntegrationCoreURL: c.cfg.IntegrationCoreURL}) != nil {
		return nil, algoliamapping.ErrUnavailable
	}
	token, err := webVitalsBearer(c.cfg.IntegrationBearerTokenFile, c.registry.OrganizationID())
	if err != nil {
		return nil, algoliamapping.ErrUnavailable
	}
	raw, err := json.Marshal(body)
	if err != nil {
		return nil, algoliamapping.ErrUnavailable
	}
	req, err := http.NewRequestWithContext(ctx, "POST", c.cfg.IntegrationCoreURL+path, bytes.NewReader(raw))
	if err != nil {
		return nil, algoliamapping.ErrUnavailable
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	response, err := c.client.Do(req)
	if err != nil {
		return nil, algoliamapping.ErrUnavailable
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return nil, algoliamapping.ErrUnavailable
	}
	raw, err = io.ReadAll(io.LimitReader(response.Body, sitesources.MaxReceiptBytes+1))
	if err != nil || len(raw) > sitesources.MaxReceiptBytes {
		return nil, algoliamapping.ErrUnavailable
	}
	return raw, nil
}
func (c *AlgoliaMappingClient) Access(ctx context.Context, connection string) (algoliamapping.Access, error) {
	raw, err := c.post(ctx, "/api/v1/insights/algolia/access", map[string]string{"connectionId": connection})
	if err != nil {
		return algoliamapping.Access{}, err
	}
	return algoliamapping.DecodeAccess(raw, connection)
}
func (c *AlgoliaMappingClient) Allowed(ctx context.Context, connection, app, index string) (bool, error) {
	raw, err := c.post(ctx, "/api/v1/insights/algolia/resources", map[string]string{"connectionId": connection, "applicationId": app, "indexName": index})
	if err != nil {
		return false, err
	}
	return algoliamapping.DecodeAllowed(raw, connection, app, index)
}
func (c *AlgoliaMappingClient) Report(ctx context.Context, r websites.Record, m algoliamapping.Mapping) (sitesources.Report, error) {
	fail := func() (sitesources.Report, error) { return sitesources.Report{}, algoliamapping.ErrUnavailable }
	w := r.Website
	if c.registry == nil || m.WebsiteID != w.ID || m.Site != w.Site || m.WebsiteRevision != w.Revision || !c.current(r) {
		return fail()
	}
	raw, err := c.post(ctx, "/api/v1/insights/algolia/report", map[string]any{"websiteId": w.ID, "site": w.Site, "websiteRevision": w.Revision, "connectionId": m.ConnectionID, "applicationId": m.ApplicationID, "indexName": m.IndexName})
	if err != nil {
		return fail()
	}
	data, err := sitesources.DecodeReceipt(raw)
	if err != nil || data.WebsiteID != w.ID || data.Site != w.Site || data.WebsiteRevision != w.Revision || data.Provider != "algolia" || data.Algolia == nil || data.Algolia.IndexName != m.IndexName || data.Algolia.Region != "provider" || !c.current(r) {
		return fail()
	}
	return data, nil
}
func (c *AlgoliaMappingClient) current(r websites.Record) bool {
	current, err := c.registry.Resolve(r.Website.ID, r.Website.Site)
	return err == nil && current.Website.Revision == r.Website.Revision
}
