package siteintegrations

import (
	"context"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

// InstallationConfigured reads Integration-owned installation metadata only.
// The signed organization connection is the authority, never a browser header.
func (b *Broker) InstallationConfigured(ctx context.Context, w websites.Website, provider string) (bool, error) {
	body := map[string]any{"websiteId": w.ID, "site": w.Site, "websiteRevision": w.Revision}
	path := "/api/v1/insights/site-sources/binding"
	if provider == "web_vitals" {
		path = "/api/v1/insights/web-vitals/binding"
		body["environment"] = "production"
	} else {
		body["provider"] = provider
	}
	var envelope struct {
		Success bool `json:"success"`
		Data    struct {
			Configured bool `json:"configured"`
		} `json:"data"`
	}
	err := b.request(ctx, "POST", path, body, &envelope)
	if err == nil && !envelope.Success {
		err = errors.New("installation authority unavailable")
	}
	return envelope.Success && envelope.Data.Configured, err
}
