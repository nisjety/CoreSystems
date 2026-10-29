package siteintegrations

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"sort"
	"strings"
	"time"
)

// IDs identify a provider resource independently of rotating connections. They are selectors,
// never credentials or permission proofs; selection always rediscovers access.
type Resource struct {
	ID       string
	Provider string
	Label    string
	Mapping  Mapping
}
type Resources struct {
	Items     []Resource
	Partial   bool
	Status    string
	CheckedAt string
}

func resource(provider, label string, m Mapping) Resource {
	hash := sha256.Sum256([]byte(provider + "\x00" + m.Property + "\x00" + m.PropertyID + "\x00" + m.StreamID))
	return Resource{"res_" + hex.EncodeToString(hash[:16]), provider, label, m}
}
func (b *Broker) GoogleResources(ctx context.Context, site string) (Resources, error) {
	out := Resources{Items: []Resource{}, Status: "needs_connection", CheckedAt: time.Now().UTC().Format(time.RFC3339)}
	if canonical, err := pulse.CanonicalSite(site); err != nil || canonical != site {
		return out, errUnavailable
	}
	ctx, cancel := context.WithTimeout(ctx, 8*time.Second)
	defer cancel()
	var envelope struct {
		Success bool `json:"success"`
		Data    struct {
			Connections []connection `json:"connections"`
		} `json:"data"`
	}
	if b.request(ctx, "GET", "/api/v1/connections?providerKey=google", nil, &envelope) != nil || !envelope.Success || len(envelope.Data.Connections) > 20 {
		return out, errUnavailable
	}
	connections := envelope.Data.Connections
	sort.Slice(connections, func(i, j int) bool { return connections[i].ID < connections[j].ID })
	seen := map[string]bool{}
	add := func(v Resource) {
		key := v.Provider + "|" + v.Mapping.Property + "|" + v.Mapping.PropertyID + "|" + v.Mapping.StreamID
		if !seen[key] && len(out.Items) < 100 {
			seen[key] = true
			out.Items = append(out.Items, v)
		} else if len(out.Items) >= 100 {
			out.Partial = true
		}
	}
	active, authorizedCount := 0, 0
	for _, c := range connections {
		if c.ProviderKey != "google" || c.Status != "active" || !validID(c.ID) {
			continue
		}
		active++
		if authorized(c, "search_console.read", "https://www.googleapis.com/auth/webmasters.readonly", "https://www.googleapis.com/auth/webmasters") {
			authorizedCount++
			var sites struct {
				Entries []struct {
					URL        string `json:"siteUrl"`
					Permission string `json:"permissionLevel"`
				} `json:"siteEntry"`
			}
			if b.action(ctx, c.ID, "gsc.sites.list", nil, &sites) != nil || len(sites.Entries) > 5000 {
				out.Partial = true
			} else {
				for _, p := range sites.Entries {
					if pulse.GSCPropertyForSiteAllowed(p.URL, site) && has([]string{"siteOwner", "siteFullUser", "siteRestrictedUser"}, p.Permission) {
						add(resource("search_console", p.URL, Mapping{Status: "connected", ConnectionID: c.ID, Property: p.URL, AccessLevel: p.Permission, CheckedAt: out.CheckedAt}))
					}
				}
			}
		}
		if authorized(c, "analytics.read", analyticsScope, "https://www.googleapis.com/auth/analytics.edit") {
			authorizedCount++
			properties, cut, err := b.analyticsProperties(ctx, c.ID)
			out.Partial = out.Partial || cut || err != nil
			if err == nil {
				for _, property := range properties {
					matches, cut, err := b.analyticsStreams(ctx, c.ID, property, site, out.CheckedAt)
					out.Partial = out.Partial || cut || err != nil
					for _, m := range matches {
						add(resource("ga4", "GA4 · "+m.PropertyID+" / "+m.StreamID, m))
					}
				}
			}
		}
		if ctx.Err() != nil {
			out.Partial = true
			break
		}
	}
	switch {
	case out.Partial:
		out.Status = "partial"
	case len(out.Items) > 0:
		out.Status = "ready"
	case active == 0:
		out.Status = "needs_connection"
	case authorizedCount == 0:
		out.Status = "requires_consent"
	default:
		out.Status = "not_found"
	}
	sort.Slice(out.Items, func(i, j int) bool {
		a, b := out.Items[i], out.Items[j]
		return strings.Compare(a.Provider+a.Label, b.Provider+b.Label) < 0
	})
	return out, nil
}

// Resolve only the explicitly selected stream's reporting host. Never perform
// this metrics probe for every account discovered during onboarding.
func (b *Broker) ResolveGoogleResource(ctx context.Context, site string, m Mapping) (Mapping, error) {
	if m.PropertyID == "" {
		return m, nil
	}
	var resolved struct {
		ReportingSite string `json:"reportingSite"`
	}
	if b.action(ctx, m.ConnectionID, "ga4.reporting_host.resolve", map[string]any{"site": site, "propertyId": m.PropertyID, "streamId": m.StreamID}, &resolved) != nil || !pulse.GA4ReportingSiteAllowed(site, resolved.ReportingSite) {
		return Mapping{}, errUnavailable
	}
	if resolved.ReportingSite != site {
		m.ReportingSite = resolved.ReportingSite
	}
	return m, nil
}
