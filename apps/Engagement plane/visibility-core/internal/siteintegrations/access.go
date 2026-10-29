package siteintegrations

import (
	"context"
	"net/url"
	"regexp"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type AccessEvidence struct {
	SearchConsole, GA4, Clarity      bool
	ClarityProjectID, ClarityVersion string
}

var bindingVersionPattern = regexp.MustCompile(`^[a-f0-9]{64}$`)
var projectPattern = regexp.MustCompile(`^[a-z0-9]{4,32}$`)

type connectionAccess struct {
	connection
	OrganizationID string  `json:"organizationId"`
	DeletedAt      *string `json:"deletedAt"`
}

// CurrentAccess checks Integration's authoritative metadata, never a provider
// analytics operation. Unreachable/malformed/revoked authority fails closed.
// External permission revocation not yet observed by Integration is bounded by
// snapshot expiry, not claimed to be synchronously known here.
func (b *Broker) CurrentAccess(ctx context.Context, org string, record websites.Record) AccessEvidence {
	var out AccessEvidence
	site, err := pulse.CanonicalSite(record.Website.Site)
	if err != nil || site != record.Website.Site || !validID(org) {
		return out
	}
	ctx, cancel := context.WithTimeout(ctx, 3*time.Second)
	defer cancel()
	loaded := map[string]connectionAccess{}
	load := func(id string) connectionAccess {
		if prior, ok := loaded[id]; ok {
			return prior
		}
		var envelope struct {
			Success bool `json:"success"`
			Data    struct {
				Connection connectionAccess `json:"connection"`
			} `json:"data"`
		}
		if !validID(id) || b.request(ctx, "GET", "/api/v1/connections/"+id, nil, &envelope) != nil || !envelope.Success {
			return connectionAccess{}
		}
		v := envelope.Data.Connection
		if v.ID != id || v.OrganizationID != org || v.ProviderKey != "google" || v.Status != "active" || v.DeletedAt != nil {
			return connectionAccess{}
		}
		loaded[id] = v
		return v
	}
	if record.Website.Integrations.SearchConsole.Status == "connected" {
		out.SearchConsole = authorized(load(record.GSCConnectionID).connection, "search_console.read", "https://www.googleapis.com/auth/webmasters.readonly", "https://www.googleapis.com/auth/webmasters")
	}
	if record.Website.Integrations.GA4.Status == "connected" {
		out.GA4 = authorized(load(record.GA4ConnectionID).connection, "analytics.read", analyticsScope, "https://www.googleapis.com/auth/analytics.edit")
	}
	var clarity struct {
		Success bool `json:"success"`
		Data    struct {
			Site       string `json:"site"`
			Configured bool   `json:"configured"`
			ProjectID  string `json:"projectId"`
			Version    string `json:"version"`
		} `json:"data"`
	}
	if b.request(ctx, "GET", "/api/v1/insights/clarity/binding?site="+url.QueryEscape(site), nil, &clarity) == nil && clarity.Success && clarity.Data.Configured && clarity.Data.Site == site && projectPattern.MatchString(clarity.Data.ProjectID) && bindingVersionPattern.MatchString(clarity.Data.Version) && (record.Website.Integrations.Clarity.ProjectID == "" || record.Website.Integrations.Clarity.ProjectID == clarity.Data.ProjectID) {
		out.Clarity = true
		out.ClarityProjectID = clarity.Data.ProjectID
		out.ClarityVersion = clarity.Data.Version
	}
	return out
}
