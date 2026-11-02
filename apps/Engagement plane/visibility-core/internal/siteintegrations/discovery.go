package siteintegrations

import (
	"context"
	"strings"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func (b *Broker) discoverSearchConsole(ctx context.Context, connections []connection, site, checked string) Mapping {
	result := Mapping{Status: "needs_connection", CheckedAt: checked}
	best := 0
	failed := false
	for _, item := range connections {
		if !authorized(item, "search_console.read", "https://www.googleapis.com/auth/webmasters.readonly", "https://www.googleapis.com/auth/webmasters") {
			continue
		}
		if result.Status == "needs_connection" {
			result.Status = "not_found"
		}
		var sites struct {
			Entries []struct {
				SiteURL    string `json:"siteUrl"`
				Permission string `json:"permissionLevel"`
			} `json:"siteEntry"`
		}
		if b.action(ctx, item.ID, "gsc.sites.list", nil, &sites) != nil || len(sites.Entries) > 5000 {
			failed = true
			continue
		}
		for _, entry := range sites.Entries {
			if !has([]string{"siteOwner", "siteFullUser", "siteRestrictedUser"}, entry.Permission) {
				continue
			}
			score := 0
			if entry.SiteURL == site {
				score = 2
			} else if pulse.GSCPropertyForSiteAllowed(entry.SiteURL, site) && strings.HasPrefix(entry.SiteURL, "sc-domain:") {
				score = 1
			} else if pulse.GSCPropertyForSiteAllowed(entry.SiteURL, site) {
				score = 1
			}
			// Read permission is not ownership. Preserve an actual owner proof
			// when the account also has a reader-only exact-prefix property.
			if score > 0 && entry.Permission == "siteOwner" {
				score += 2
			}
			if score > best {
				best = score
				result = Mapping{Status: "connected", ConnectionID: item.ID, Property: entry.SiteURL, AccessLevel: entry.Permission, CheckedAt: checked}
			}
		}
	}
	if result.Status != "connected" && failed {
		result.Status = "source_unavailable"
	}
	return result
}

type accountPage struct {
	Accounts []struct {
		Properties []struct {
			Property string `json:"property"`
		} `json:"propertySummaries"`
	} `json:"accountSummaries"`
	Next string `json:"nextPageToken"`
}
type streamPage struct {
	Streams []struct {
		Name string `json:"name"`
		Type string `json:"type"`
		Web  struct {
			DefaultURI    string `json:"defaultUri"`
			MeasurementID string `json:"measurementId"`
		} `json:"webStreamData"`
	} `json:"dataStreams"`
	Next string `json:"nextPageToken"`
}

func (b *Broker) discoverAnalytics(ctx context.Context, connections []connection, site, checked string) Mapping {
	result := Mapping{Status: "requires_consent", CheckedAt: checked}
	matches := map[string]Mapping{}
	failed, truncated := false, false
	for _, item := range connections {
		if !authorized(item, "analytics.read", analyticsScope, "https://www.googleapis.com/auth/analytics.edit") {
			continue
		}
		result.Status = "not_found"
		properties, cut, err := b.analyticsProperties(ctx, item.ID)
		truncated = truncated || cut
		if err != nil {
			failed = true
			continue
		}
		for _, property := range properties {
			found, cut, err := b.analyticsStreams(ctx, item.ID, property, site, checked)
			truncated = truncated || cut
			if err != nil {
				failed = true
				continue
			}
			for _, mapping := range found {
				key := mapping.PropertyID + "/" + mapping.StreamID
				if _, exists := matches[key]; !exists {
					matches[key] = mapping
				}
			}
		}
	}
	if len(matches) > 1 || truncated {
		return Mapping{Status: "needs_selection", CheckedAt: checked}
	}
	// An incomplete scan cannot establish that a single match is unambiguous.
	if failed {
		return Mapping{Status: "source_unavailable", CheckedAt: checked}
	}
	for _, mapping := range matches {
		var resolved struct {
			ReportingSite string `json:"reportingSite"`
		}
		if b.action(ctx, mapping.ConnectionID, "ga4.reporting_host.resolve", map[string]any{"site": site, "propertyId": mapping.PropertyID, "streamId": mapping.StreamID}, &resolved) != nil || !pulse.GA4ReportingSiteAllowed(site, resolved.ReportingSite) {
			return Mapping{Status: "source_unavailable", CheckedAt: checked}
		}
		if resolved.ReportingSite != site {
			mapping.ReportingSite = resolved.ReportingSite
		}
		return mapping
	}
	return result
}

func (b *Broker) analyticsProperties(ctx context.Context, id string) ([]string, bool, error) {
	properties := []string{}
	seen := map[string]bool{}
	token := ""
	for page := 0; page < 3; page++ {
		var result accountPage
		params := map[string]any{"pageSize": 100}
		if token != "" {
			params["pageToken"] = token
		}
		if b.action(ctx, id, "ga4.account_summaries.list", params, &result) != nil || len(result.Accounts) > 100 {
			return nil, false, errUnavailable
		}
		for _, account := range result.Accounts {
			for _, property := range account.Properties {
				propertyID := strings.TrimPrefix(property.Property, "properties/")
				if property.Property != "properties/"+propertyID || !numericID(propertyID) {
					return nil, false, errUnavailable
				}
				if !seen[propertyID] {
					if len(properties) >= 20 {
						return properties, true, nil
					}
					seen[propertyID] = true
					properties = append(properties, propertyID)
				}
			}
		}
		if result.Next == "" {
			return properties, false, nil
		}
		if len(result.Next) > 2048 || result.Next == token {
			return nil, false, errUnavailable
		}
		token = result.Next
	}
	return properties, true, nil
}

func (b *Broker) analyticsStreams(ctx context.Context, id, property, site, checked string) ([]Mapping, bool, error) {
	matches := []Mapping{}
	token := ""
	for page := 0; page < 2; page++ {
		var result streamPage
		params := map[string]any{"propertyId": property, "pageSize": 100}
		if token != "" {
			params["pageToken"] = token
		}
		if b.action(ctx, id, "ga4.data_streams.list", params, &result) != nil || len(result.Streams) > 100 {
			return nil, false, errUnavailable
		}
		for _, stream := range result.Streams {
			if stream.Type != "WEB_DATA_STREAM" {
				continue
			}
			canonical, err := pulse.CanonicalSite(stream.Web.DefaultURI)
			if err != nil || !pulse.GA4ReportingSiteAllowed(site, canonical) {
				continue
			}
			prefix := "properties/" + property + "/dataStreams/"
			streamID := strings.TrimPrefix(stream.Name, prefix)
			if !strings.HasPrefix(stream.Name, prefix) || !numericID(streamID) || !measurementID(stream.Web.MeasurementID) {
				return nil, false, errUnavailable
			}
			matches = append(matches, Mapping{Status: "connected", ConnectionID: id, PropertyID: property, StreamID: streamID, MeasurementID: stream.Web.MeasurementID, CheckedAt: checked})
		}
		if result.Next == "" {
			return matches, false, nil
		}
		if len(result.Next) > 2048 || result.Next == token {
			return nil, false, errUnavailable
		}
		token = result.Next
	}
	return matches, true, nil
}
func numericID(value string) bool {
	if len(value) < 1 || len(value) > 30 {
		return false
	}
	for _, char := range value {
		if char < '0' || char > '9' {
			return false
		}
	}
	return true
}
func measurementID(value string) bool {
	if !strings.HasPrefix(value, "G-") || len(value) < 3 || len(value) > 32 {
		return false
	}
	for _, char := range value[2:] {
		if !(char >= 'A' && char <= 'Z' || char >= '0' && char <= '9') {
			return false
		}
	}
	return true
}
