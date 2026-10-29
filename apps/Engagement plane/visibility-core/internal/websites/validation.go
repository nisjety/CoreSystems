package websites

import (
	"regexp"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

var measurementPattern = regexp.MustCompile(`^G-[A-Z0-9]{5,30}$`)

func validOwnership(v Ownership) bool {
	if v.Status != "unverified" && v.Status != "google_owner" && v.Status != "dns_verified" {
		return false
	}
	return v.CheckedAt == "" && v.Status == "unverified" || timestamp(v.CheckedAt)
}
func validMappings(r Record) bool {
	i := r.Website.Integrations
	if i.PageSpeed.Status != "available" || i.PageSpeed != (Mapping{Status: "available"}) {
		return false
	}
	if i.Clarity.Status != "not_connected" && i.Clarity.Status != "pending_verification" {
		return false
	}
	if i.Clarity.Status == "pending_verification" && !clarityPattern.MatchString(i.Clarity.ProjectID) {
		return false
	}
	if i.Clarity.Status == "not_connected" && i.Clarity.ProjectID != "" {
		return false
	}
	if !statusIn(i.SearchConsole.Status, "unmapped", "connected", "needs_connection", "not_found", "source_unavailable") {
		return false
	}
	if !statusIn(i.GA4.Status, "not_connected", "requires_consent", "needs_selection", "connected", "not_found", "source_unavailable") {
		return false
	}
	if i.SearchConsole.CheckedAt != "" && !timestamp(i.SearchConsole.CheckedAt) || i.GA4.CheckedAt != "" && !timestamp(i.GA4.CheckedAt) {
		return false
	}
	if i.SearchConsole.Status == "connected" {
		if !identityPattern.MatchString(r.GSCConnectionID) || !propertyForSite(i.SearchConsole.Property, r.Website.Site) || !statusIn(i.SearchConsole.AccessLevel, "siteOwner", "siteFullUser", "siteRestrictedUser") {
			return false
		}
	} else if r.GSCConnectionID != "" || i.SearchConsole.Property != "" || i.SearchConsole.AccessLevel != "" {
		return false
	}
	if i.GA4.Status == "connected" {
		if !identityPattern.MatchString(r.GA4ConnectionID) || !numericPattern.MatchString(i.GA4.PropertyID) || !numericPattern.MatchString(i.GA4.StreamID) || !measurementPattern.MatchString(i.GA4.MeasurementID) {
			return false
		}
	} else if r.GA4ConnectionID != "" || i.GA4.PropertyID != "" || i.GA4.StreamID != "" || i.GA4.MeasurementID != "" {
		return false
	}
	if i.GA4.ReportingSite != "" && (!pulse.GA4ReportingSiteAllowed(r.Website.Site, i.GA4.ReportingSite) || i.GA4.ReportingSite == r.Website.Site || i.GA4.Status != "connected" || !identityPattern.MatchString(r.GA4ConnectionID)) {
		return false
	}
	if i.SearchConsole.ReportingSite != "" || i.Clarity.ReportingSite != "" || i.PageSpeed.ReportingSite != "" {
		return false
	}
	return r.Website.Ownership.Status != "google_owner" || i.SearchConsole.Status == "connected" && i.SearchConsole.AccessLevel == "siteOwner"
}

func statusIn(value string, allowed ...string) bool {
	for _, candidate := range allowed {
		if value == candidate {
			return true
		}
	}
	return false
}
func propertyForSite(property, site string) bool {
	return pulse.GSCPropertyForSiteAllowed(property, site)
}
