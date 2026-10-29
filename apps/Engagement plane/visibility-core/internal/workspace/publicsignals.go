package workspace

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"os"
	"strings"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/publicsignals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type PublicSignalsCollector struct {
	cfg      pulse.Config
	registry *websites.Service
	client   *http.Client
}

func NewPublicSignalsCollector(cfg pulse.Config, registry *websites.Service) *PublicSignalsCollector {
	return &PublicSignalsCollector{cfg: cfg, registry: registry, client: &http.Client{Timeout: 35 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}

// Collect is called only by background jobs. Integration validates the signed
// org bearer independently, owns provider requests, and never leases its key.
func (c *PublicSignalsCollector) Collect(ctx context.Context, record websites.Record) (publicsignals.Report, error) {
	failure := func() (publicsignals.Report, error) { return publicsignals.Report{}, publicsignals.ErrInvalid }
	if c.registry == nil || pulse.ValidateConfig(pulse.Config{IntegrationCoreURL: c.cfg.IntegrationCoreURL}) != nil || !boundToOrganization(c.cfg.IntegrationBearerTokenFile, c.registry.OrganizationID()) {
		return failure()
	}
	current, err := c.registry.Resolve(record.Website.ID, record.Website.Site)
	if err != nil || current.Website.Revision != record.Website.Revision {
		return publicsignals.Report{}, publicsignals.ErrConflict
	}
	info, err := os.Lstat(c.cfg.IntegrationBearerTokenFile)
	if err != nil || !info.Mode().IsRegular() || info.Size() < 1 || info.Size() > 8192 || info.Mode().Perm()&0077 != 0 {
		return failure()
	}
	raw, err := os.ReadFile(c.cfg.IntegrationBearerTokenFile)
	token := strings.TrimSpace(string(raw))
	if err != nil || len(raw) > 8192 || token == "" || strings.ContainsAny(token, " \r\n\t") {
		return failure()
	}
	body, _ := json.Marshal(map[string]string{"site": record.Website.Site})
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.cfg.IntegrationCoreURL+"/api/v1/insights/public-signals", bytes.NewReader(body))
	if err != nil {
		return failure()
	}
	req.Header.Set("Authorization", "Bearer "+token)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Accept", "application/json")
	response, err := c.client.Do(req)
	if err != nil {
		return failure()
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return failure()
	}
	payload, err := io.ReadAll(io.LimitReader(response.Body, (128<<10)+1))
	if err != nil || len(payload) > 128<<10 {
		return failure()
	}
	var envelope struct {
		Success bool                 `json:"success"`
		Data    publicsignals.Report `json:"data"`
	}
	if json.Unmarshal(payload, &envelope) != nil || !envelope.Success || !publicsignals.ValidReport(envelope.Data, record.Website.Site, time.Now()) {
		return failure()
	}
	current, err = c.registry.Resolve(record.Website.ID, record.Website.Site)
	if err != nil || current.Website.Revision != record.Website.Revision {
		return publicsignals.Report{}, publicsignals.ErrConflict
	}
	return envelope.Data, nil
}
