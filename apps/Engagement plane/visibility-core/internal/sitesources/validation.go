package sitesources

import (
	"math"
	"net/url"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func count(v int64) bool         { return v >= 0 && v <= 9007199254740991 }
func countPointer(v *int64) bool { return v != nil && count(*v) }
func component(status string) bool {
	switch status {
	case "connected", "no_data", "access_denied", "plan_required", "rate_limited", "unavailable":
		return true
	}
	return false
}
func successful(status string) bool { return status == "connected" || status == "no_data" }
func validReport(r Report) bool {
	canonical, err := pulse.CanonicalSite(r.Site)
	if err != nil || canonical != r.Site || !idPattern.MatchString(r.WebsiteID) || r.WebsiteRevision < 1 || r.WebsiteRevision > 9007199254740991 || !provider(r.Provider) {
		return false
	}
	observed, err := time.Parse(time.RFC3339Nano, r.ObservedAt)
	if err != nil || r.Window != period(observed) {
		return false
	}
	if r.Status == "not_configured" || r.Status == "not_loaded" {
		return r.Cookiebot == nil && r.Algolia == nil
	}
	if r.Provider == "cookiebot" {
		if r.Cookiebot == nil || r.Algolia != nil || !validCookiebot(*r.Cookiebot, r.Site, observed) {
			return false
		}
		successes := 0
		if successful(r.Cookiebot.Consents.Status) {
			successes++
		}
		if successful(r.Cookiebot.Inventory.Status) {
			successes++
		}
		return r.Status == []string{"unavailable", "partial", "connected"}[successes]
	}
	if r.Algolia == nil || r.Cookiebot != nil || !indexPattern.MatchString(r.Algolia.IndexName) || r.Algolia.Region != "us" && r.Algolia.Region != "de" && r.Algolia.Region != "provider" || !validAnalytics(r.Algolia.Analytics) {
		return false
	}
	if !validTerms(r.Algolia.TopSearches) || !validTerms(r.Algolia.NoResultTerms) {
		return false
	}
	if successful(r.Algolia.Analytics.Status) || connectedTerms(r.Algolia.TopSearches) || connectedTerms(r.Algolia.NoResultTerms) {
		return r.Status == "connected"
	}
	return r.Status == "unavailable"
}
func connectedTerms(t *Terms) bool { return t != nil && t.Status == "connected" }
func validTerms(t *Terms) bool {
	if t == nil {
		return true
	} // Legacy aggregate-only snapshots mean unavailable terms.
	if t.Limit != 20 {
		return false
	}
	if t.Status == "unavailable" {
		return t.Rows == nil
	}
	if t.Status != "connected" || t.Rows == nil || len(t.Rows) > 20 {
		return false
	}
	seen := map[string]bool{}
	for _, row := range t.Rows {
		if !utf8.ValidString(row.Query) || len(row.Query) > 2048 || utf8.RuneCountInString(row.Query) > 512 || seen[row.Query] || !count(row.Count) {
			return false
		}
		for _, r := range row.Query {
			if unicode.IsControl(r) {
				return false
			}
		}
		seen[row.Query] = true
		if row.AverageResults != nil && (math.IsNaN(*row.AverageResults) || math.IsInf(*row.AverageResults, 0) || *row.AverageResults < 0 || *row.AverageResults > 9007199254740991) {
			return false
		}
	}
	return true
}
func validCookiebot(c Cookiebot, site string, observed time.Time) bool {
	u, err := url.Parse(site)
	if err != nil {
		return false
	}
	base := strings.TrimPrefix(u.Host, "www.")
	if c.ReportingDomain != base && c.ReportingDomain != "www."+base {
		return false
	}
	return validConsents(c.Consents) && validInventory(c.Inventory, observed)
}
func validConsents(c Consents) bool {
	if !component(c.Status) || c.ReportedDays < 0 || c.ReportedDays > 7 {
		return false
	}
	if c.Status != "connected" {
		return c.ReportedDays == 0 && c.Totals == nil
	}
	if c.ReportedDays < 1 || c.Totals == nil {
		return false
	}
	t := c.Totals
	return count(t.OptIn) && count(t.OptOut) && count(t.Preferences) && count(t.Statistics) && count(t.Marketing)
}
func validInventory(i Inventory, observed time.Time) bool {
	if !component(i.Status) {
		return false
	}
	if i.Status != "connected" {
		return i.ScannedAt == "" && i.Total == nil && i.Categories == nil && i.ThirdParty == nil && i.Secure == nil && i.HTTPOnly == nil
	}
	if !countPointer(i.Total) || i.Categories == nil || !countPointer(i.ThirdParty) || !countPointer(i.Secure) || !countPointer(i.HTTPOnly) {
		return false
	}
	c := i.Categories
	if !count(c.Necessary) || !count(c.Preferences) || !count(c.Statistics) || !count(c.Marketing) || !count(c.Unclassified) || c.Necessary+c.Preferences+c.Statistics+c.Marketing+c.Unclassified != *i.Total || *i.ThirdParty > *i.Total || *i.Secure > *i.Total || *i.HTTPOnly > *i.Total {
		return false
	}
	at, err := time.Parse(time.RFC3339Nano, i.ScannedAt)
	return err == nil && !at.After(observed)
}
func validAnalytics(a Analytics) bool {
	if !component(a.Status) || a.ReportedDays < 0 || a.ReportedDays > 7 {
		return false
	}
	if a.Status != "connected" {
		return a.ReportedDays == 0 && a.Searches == nil && a.NoResultSearches == nil && a.NoResultRate == nil
	}
	if !countPointer(a.Searches) || !countPointer(a.NoResultSearches) || *a.NoResultSearches > *a.Searches {
		return false
	}
	if *a.Searches == 0 {
		return *a.NoResultSearches == 0 && a.NoResultRate == nil
	}
	if a.ReportedDays < 1 || a.NoResultRate == nil || math.IsNaN(*a.NoResultRate) || math.IsInf(*a.NoResultRate, 0) || *a.NoResultRate < 0 || *a.NoResultRate > 1 {
		return false
	}
	return math.Abs(*a.NoResultRate-float64(*a.NoResultSearches)/float64(*a.Searches)) <= 1e-6
}
