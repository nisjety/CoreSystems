package analyticssync

import (
	"encoding/json"
	"math"
	"net/url"
	"regexp"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

func validateState(v diskState) error {
	if v.Version != 1 || !orgPattern.MatchString(v.OrganizationID) || v.Entries == nil || len(v.Entries) > maxEntries {
		return ErrInvalid
	}
	seen := map[string]bool{}
	for _, e := range v.Entries {
		key := e.WebsiteID + e.Quarter
		canonical, err := pulse.CanonicalSite(e.Site)
		if seen[key] || !idPattern.MatchString(e.WebsiteID) || err != nil || canonical != e.Site || e.WebsiteRevision < 1 || e.Source != "marketing_analytics_sync" || !fingerprintPattern.MatchString(e.Fingerprint) || e.Year < 2020 || e.Year > 9999 || e.Attempts < 0 || e.Attempts > 3 || !validQuarter(e.Quarter) || !jobPattern.MatchString(e.JobID) || len(e.Steps) != 3 {
			return ErrInvalid
		}
		seen[key] = true
		if e.Status != "queued" && e.Status != "running" && e.Status != "complete" && e.Status != "partial" && e.Status != "failed" {
			return ErrInvalid
		}
		q := parsed(e.QueuedAt)
		if q.IsZero() {
			return ErrInvalid
		}
		for _, v := range []string{e.StartedAt, e.CompletedAt, e.NextSyncAt, e.ExpiresAt} {
			if v != "" && parsed(v).IsZero() {
				return ErrInvalid
			}
		}
		if e.StartedAt != "" && parsed(e.StartedAt).Before(q) {
			return ErrInvalid
		}
		if e.Status == "running" && (e.StartedAt == "" || e.Attempts == 0) {
			return ErrInvalid
		}
		terminal := e.Status == "complete" || e.Status == "partial" || e.Status == "failed"
		if terminal && (e.StartedAt == "" || e.CompletedAt == "" || parsed(e.CompletedAt).Before(parsed(e.StartedAt))) {
			return ErrInvalid
		}
		if !terminal && (e.CompletedAt != "" || e.ExpiresAt != "" || e.Bundle != nil) {
			return ErrInvalid
		}
		connected := 0
		for i, step := range e.Steps {
			if step.Key != stepKeys[i] {
				return ErrInvalid
			}
			if step.LastSuccessAt != "" && (parsed(step.LastSuccessAt).IsZero() || terminal && parsed(step.LastSuccessAt).After(parsed(e.CompletedAt)) || !terminal && parsed(step.LastSuccessAt).After(q)) {
				return ErrInvalid
			}
			switch step.Status {
			case "queued", "running":
				if terminal || step.Reason != "" || step.ObservedAt != "" {
					return ErrInvalid
				}
			case "connected", "unavailable":
				if parsed(step.ObservedAt).IsZero() || step.Status == "connected" && step.Reason != "" || step.Status == "unavailable" && !collectionReason(step.Reason) {
					return ErrInvalid
				}
				if step.Status == "connected" {
					connected++
				}
			default:
				return ErrInvalid
			}
		}
		if terminal && (e.Status == "complete" && connected != 3 || e.Status == "partial" && (connected == 0 || connected == 3) || e.Status == "failed" && connected != 0) {
			return ErrInvalid
		}
		if e.Bundle != nil {
			observed := parsed(e.CompletedAt)
			if e.ExpiresAt == "" || parsed(e.ExpiresAt).After(observed.Add(metricsTTL)) || !validBundle(*e.Bundle, e.Site, e.Quarter, observed) {
				return ErrInvalid
			}
		}
	}
	return nil
}
func validPeriod(p pulse.Period, q string, now time.Time) bool {
	want, err := pulse.QuarterPeriod(q, now)
	if err != nil || p.StartDate != want.StartDate {
		return false
	}
	end, err := time.Parse("2006-01-02", p.EndDate)
	return err == nil && p.EndDate >= p.StartDate && p.EndDate <= want.EndDate && !end.IsZero()
}
func finite(v float64) bool {
	return !math.IsNaN(v) && !math.IsInf(v, 0) && v >= 0 && v <= 9007199254740991
}
func count(v int64) bool { return v >= 0 && v <= 9007199254740991 }
func sourceStatus(status, why, updated string, now time.Time) bool {
	if status == "connected" {
		return why == "" && !parsed(updated).IsZero() && !parsed(updated).After(now)
	}
	return status == "unavailable" && reason(why) && (updated == "" || !parsed(updated).IsZero() && !parsed(updated).After(now))
}
func validBundle(b Bundle, site, q string, now time.Time) bool {
	if raw, err := json.Marshal(b); err != nil || len(raw) > maxBytes/2 {
		return false
	}
	p, s, g, c := b.Pulse, b.Search, b.GA4, b.Clarity
	if p.Site != site || s.Site != site || g.Site != site || c.Site != site || p.Source != "google_search_console" || s.Source != "google_search_console" || g.Source != "google_analytics" || c.Source != "microsoft_clarity" || !validPeriod(p.Period, q, now) || !validPeriod(s.Period, q, now) || !validPeriod(g.Period, q, now) {
		return false
	}
	if !sourceStatus(p.Status, p.Reason, p.UpdatedAt, now) || !sourceStatus(s.Status, s.Reason, s.UpdatedAt, now) || !sourceStatus(g.Status, g.Reason, g.UpdatedAt, now) || !sourceStatus(c.Status, c.Reason, c.UpdatedAt, now) {
		return false
	}
	maxDays := 92
	if q == "FY" {
		maxDays = 366
	}
	if len(p.Days) > maxDays || len(g.Days) > maxDays || len(s.Queries) > 100 || len(s.Pages) > 100 || len(s.Opportunities) > 100 || s.RowLimit != 100 || c.Scope != "clarity_project" || c.Window.NumOfDays != 3 || c.Window.TimeZone != "UTC" {
		return false
	}
	if p.Status == "connected" {
		if p.Totals == nil || !count(p.Totals.Clicks) || !count(p.Totals.Impressions) || p.Totals.Clicks > p.Totals.Impressions || !validScope(p.PropertyScope) {
			return false
		}
	} else if p.Totals != nil || len(p.Days) > 0 {
		return false
	}
	previous := ""
	for _, d := range p.Days {
		if !day(d.Date, p.Period, previous) || !count(d.Clicks) || !count(d.Impressions) || d.Clicks > d.Impressions {
			return false
		}
		previous = d.Date
	}
	if s.Status == "connected" && !validScope(s.PropertyScope) {
		return false
	}
	for _, source := range []struct{ property, status, scope string }{{p.ReportingProperty, p.Status, p.PropertyScope}, {s.ReportingProperty, s.Status, s.PropertyScope}} {
		if source.property != "" && (source.status != "connected" || !pulse.GSCPropertyForSiteAllowed(source.property, site) || strings.HasPrefix(source.property, "sc-domain:") != (source.scope == "domain")) {
			return false
		}
	}
	if p.ReportingProperty != "" && s.ReportingProperty != "" && p.ReportingProperty != s.ReportingProperty {
		return false
	}
	if s.QueriesAtLimit != (len(s.Queries) == 100) || s.PagesAtLimit != (len(s.Pages) == 100) {
		return false
	}
	if s.Status != "connected" && (len(s.Queries) > 0 || len(s.Pages) > 0 || len(s.Opportunities) > 0) {
		return false
	}
	for dimension, rows := range [][]pulse.SearchRow{s.Queries, s.Pages} {
		seen := map[string]bool{}
		for _, r := range rows {
			reportingSite := site
			if s.ReportingProperty != "" {
				if strings.HasPrefix(s.ReportingProperty, "sc-domain:") {
					reportingSite = "https://" + strings.TrimPrefix(s.ReportingProperty, "sc-domain:") + "/"
				} else {
					reportingSite = s.ReportingProperty
				}
			}
			if !searchRow(r) || seen[r.Key] || !searchKey(r.Key, reportingSite, s.PropertyScope, dimension == 1) {
				return false
			}
			seen[r.Key] = true
		}
	}
	seenOpportunities := map[string]bool{}
	for _, r := range s.Opportunities {
		if !searchRow(r.SearchRow) || !searchKey(r.Key, site, s.PropertyScope, false) || seenOpportunities[r.Key] || !opportunityType(r.Type) {
			return false
		}
		seenOpportunities[r.Key] = true
		found := false
		for _, query := range s.Queries {
			if query == r.SearchRow {
				found = true
				break
			}
		}
		if !found {
			return false
		}
	}
	if g.ReportingSite != "" && !pulse.GA4ReportingSiteAllowed(site, g.ReportingSite) {
		return false
	}
	if g.Status == "connected" {
		if !validZone(g.TimeZone) || g.Totals != nil && !ga4Counts(*g.Totals) || g.Totals == nil && len(g.Days) > 0 {
			return false
		}
	} else if g.Totals != nil || len(g.Days) > 0 {
		return false
	}
	previous = ""
	for _, d := range g.Days {
		if !day(d.Date, g.Period, previous) || !ga4Counts(d.GA4Metrics) {
			return false
		}
		previous = d.Date
	}
	if c.Window.ObservedAt != "" && (parsed(c.Window.ObservedAt).IsZero() || parsed(c.Window.ObservedAt).After(now)) {
		return false
	}
	if c.Status == "connected" {
		if !clarityProjectPattern.MatchString(c.ProjectID) || !fingerprintPattern.MatchString(c.BindingVersion) || c.Metrics == nil || parsed(c.Window.ObservedAt).IsZero() {
			return false
		}
		m := c.Metrics
		if !pulse.ValidClarityUX(m.UX) {
			return false
		}
		if m.Sessions == nil && m.BotSessions == nil && m.Users == nil && m.PagesPerSession == nil {
			return false
		}
		for _, v := range []*int64{m.Sessions, m.BotSessions, m.Users} {
			if v != nil && !count(*v) {
				return false
			}
		}
		if m.PagesPerSession != nil && !finite(*m.PagesPerSession) {
			return false
		}
	} else if c.Metrics != nil {
		return false
	}
	return true
}

var clarityProjectPattern = regexp.MustCompile(`^[a-z0-9]{4,32}$`)

func validScope(v string) bool { return v == "domain" || v == "url_prefix" }
func day(v string, p pulse.Period, previous string) bool {
	d, err := time.Parse("2006-01-02", v)
	return err == nil && d.Format("2006-01-02") == v && v >= p.StartDate && v <= p.EndDate && v > previous
}
func searchRow(r pulse.SearchRow) bool {
	ctr := float64(0)
	if r.Impressions > 0 {
		ctr = float64(r.Clicks) / float64(r.Impressions)
	}
	return count(r.Clicks) && count(r.Impressions) && r.Clicks <= r.Impressions && finite(r.CTR) && r.CTR <= 1 && math.Abs(r.CTR-ctr) < 1e-12 && finite(r.Position) && (r.Impressions == 0 || r.Position >= 1)
}
func searchKey(v, site, scope string, page bool) bool {
	limit := 512
	if page {
		limit = 2048
	}
	if len(v) < 1 || len(v) > limit || strings.TrimSpace(v) == "" || !utf8.ValidString(v) || strings.ContainsFunc(v, unicode.IsControl) {
		return false
	}
	if !page {
		return true
	}
	u, err := url.Parse(v)
	base, _ := url.Parse(site)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.User != nil || u.Opaque != "" || u.Fragment != "" || u.Host != u.Hostname() || strings.ContainsAny(v, "\\ ") {
		return false
	}
	if scope == "url_prefix" {
		return u.Host == base.Host && u.Scheme == base.Scheme
	}
	return u.Host == base.Host || strings.HasSuffix(u.Host, "."+base.Host)
}
func opportunityType(v string) bool {
	return v == "content_expansion" || v == "snippet_review" || v == "title_meta" || v == "title_test"
}
func validZone(v string) bool {
	if v == "" || len(v) > 64 || v == "Local" || strings.ContainsAny(v, "\x00\r\n") {
		return false
	}
	_, err := time.LoadLocation(v)
	return err == nil
}
func ga4Counts(m pulse.GA4Metrics) bool {
	return count(m.Users) && count(m.Sessions) && count(m.EngagedSessions) && count(m.PageViews) && finite(m.KeyEvents) && finite(m.EngagementSeconds)
}

func validQuarter(q string) bool {
	return q == "FY" || len(q) == 2 && q[0] == 'Q' && q[1] >= '1' && q[1] <= '4'
}
