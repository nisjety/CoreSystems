package publicsignals

import (
	"encoding/json"
	"math"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

// ValidReport is shared by the Integration adapter and the durable boundary.
func ValidReport(r Report, site string, now time.Time) bool {
	raw, err := json.Marshal(r)
	if err != nil || len(raw) > 64<<10 || r.Site != site || r.Source != "public_web_signals" || parse(r.ObservedAt).IsZero() || parse(r.ObservedAt).After(now) || now.Sub(parse(r.ObservedAt)) > successTTL || len(r.CrUX) != 2 {
		return false
	}
	for i, c := range r.CrUX {
		factor := "PHONE"
		if i == 1 {
			factor = "DESKTOP"
		}
		if c.FormFactor != factor {
			return false
		}
		if c.Status == "unavailable" {
			if !providerReason(c.Reason) || c.Period != nil || c.Metrics != nil || c.ReportingOrigin != "" {
				return false
			}
			continue
		}
		if c.Status != "connected" || c.Reason != "" || c.Period == nil || c.Metrics == nil {
			return false
		}
		// Absent only in historical exact-origin snapshots. An explicit source
		// must be the selected origin or its one-label www counterpart.
		if c.ReportingOrigin != "" && !cruxOriginAllowed(site, c.ReportingOrigin) {
			return false
		}
		start, e1 := time.Parse("2006-01-02", c.Period.StartDate)
		end, e2 := time.Parse("2006-01-02", c.Period.EndDate)
		if e1 != nil || e2 != nil || start.Format("2006-01-02") != c.Period.StartDate || end.Format("2006-01-02") != c.Period.EndDate || end.Sub(start) != 27*24*time.Hour || c.Period.EndDate > r.ObservedAt[:10] {
			return false
		}
		m := c.Metrics
		if m.LCPMs == nil && m.INPMs == nil && m.CLS == nil {
			return false
		}
		for _, metric := range []*float64{m.LCPMs, m.INPMs, m.CLS} {
			if metric != nil && (math.IsNaN(*metric) || math.IsInf(*metric, 0) || *metric < 0 || *metric > 9007199254740991) {
				return false
			}
		}
	}
	c := r.CommonCrawl
	if c.Limit != 20 || c.Captures == nil || len(c.Captures) > 20 || c.Index != "" && !validIndex(c.Index, now) {
		return false
	}
	if c.Status == "unavailable" {
		return providerReason(c.Reason) && len(c.Captures) == 0 && !c.Truncated && c.ReportingHost == ""
	}
	if c.Status != "connected" || c.Reason != "" || c.Index == "" || len(c.Captures) == 0 {
		return false
	}
	base, _ := url.Parse(site)
	if c.ReportingHost != "" {
		if !cruxOriginAllowed(site, "https://"+c.ReportingHost) {
			return false
		}
		base.Host = c.ReportingHost
	}
	seen := map[string]bool{}
	for _, capture := range c.Captures {
		u, err := url.Parse(capture.URL)
		at := parse(capture.CapturedAt)
		key := capture.URL + "\n" + capture.CapturedAt + "\n" + strconv.Itoa(capture.HTTPStatus)
		if err != nil || len(capture.URL) > 2048 || (u.Scheme != "http" && u.Scheme != "https") || u.User != nil || u.Opaque != "" || u.Port() != "" || u.Host != base.Host || u.String() != capture.URL || u.RawQuery != "" || u.ForceQuery || u.RawFragment != "" || u.Fragment != "" || strings.ContainsAny(capture.URL, "\\\r\n\x00 ") || !safePath(u.Path) || at.IsZero() || at.Before(time.Date(2008, 1, 1, 0, 0, 0, 0, time.UTC)) || at.After(parse(r.ObservedAt)) || capture.HTTPStatus < 100 || capture.HTTPStatus > 599 || seen[key] {
			return false
		}
		seen[key] = true
	}
	return true
}

func cruxOriginAllowed(site, origin string) bool {
	canonical, err := pulse.CanonicalSite(site)
	if err != nil || canonical != site {
		return false
	}
	canonical, err = pulse.CanonicalSite(origin + "/")
	if err != nil || canonical != origin+"/" {
		return false
	}
	selected := strings.TrimSuffix(site, "/")
	if origin == selected {
		return true
	}
	host := strings.TrimPrefix(selected, "https://")
	if strings.HasPrefix(host, "www.") {
		return origin == "https://"+strings.TrimPrefix(host, "www.")
	}
	return origin == "https://www."+host
}
func validIndex(v string, now time.Time) bool {
	if !indexPattern.MatchString(v) {
		return false
	}
	year, _ := strconv.Atoi(v[8:12])
	week, _ := strconv.Atoi(v[13:])
	return year >= 2008 && year <= now.UTC().Year() && week >= 1 && week <= 53
}
func safePath(path string) bool {
	for range 4 {
		if strings.ContainsFunc(path, unicode.IsControl) || strings.Contains(path, "\\") {
			return false
		}
		if !strings.Contains(path, "%") {
			break
		}
		decoded, err := url.PathUnescape(path)
		if err != nil {
			return false
		}
		path = decoded
	}
	if strings.Contains(path, "%") || strings.ContainsFunc(path, unicode.IsControl) || strings.Contains(path, "\\") {
		return false
	}
	for _, segment := range strings.Split(strings.ToLower(path), "/") {
		segment = strings.SplitN(segment, ";", 2)[0]
		switch segment {
		case ".", "..", "auth", "account", "accounts", "profile", "settings", "dashboard", "session", "sessions", "password", "reset", "reset-password", "recover", "admin", "api", "wp-admin", "wp-login.php", "login", "logout", "signin", "sign-in", "signup", "sign-up", "register", "oauth", "token", "tokens", "secret", "secrets", "callback", "verify", "verification", "invite", "invitations", "unsubscribe", "magic-link", "payment", "payments", "checkout", "cart", "billing":
			return false
		}
	}
	return true
}
func validState(v diskState) bool {
	if v.Version != 1 || !orgPattern.MatchString(v.OrganizationID) || v.Entries == nil || len(v.Entries) > maxEntries {
		return false
	}
	seen := map[string]bool{}
	for _, s := range v.Entries {
		site, err := pulse.CanonicalSite(s.Site)
		if err != nil || site != s.Site || !idPattern.MatchString(s.WebsiteID) || seen[s.WebsiteID] || s.WebsiteRevision < 1 || s.Source != "marketing_public_signals" || !jobPattern.MatchString(s.JobID) || parse(s.QueuedAt).IsZero() {
			return false
		}
		seen[s.WebsiteID] = true
		if s.CachedReport != nil {
			boundary := parse(s.QueuedAt)
			if s.StartedAt != "" {
				boundary = parse(s.StartedAt)
			}
			if s.CompletedAt != "" {
				boundary = parse(s.CompletedAt)
			}
			if !ValidReport(*s.CachedReport, s.Site, parse(s.CachedReport.ObservedAt)) || parse(s.CachedReport.ObservedAt).After(boundary) {
				return false
			}
		}
		for _, stamp := range []string{s.StartedAt, s.CompletedAt, s.NextSyncAt, s.ExpiresAt} {
			if stamp != "" && parse(stamp).IsZero() {
				return false
			}
		}
		if s.StartedAt != "" && parse(s.StartedAt).Before(parse(s.QueuedAt)) {
			return false
		}
		if s.Status == "queued" {
			if s.StartedAt != "" || s.CompletedAt != "" || s.ExpiresAt != "" || s.NextSyncAt != "" || s.Report != nil || s.Reason != "" {
				return false
			}
			continue
		}
		if s.Status == "running" {
			if s.StartedAt == "" || s.CompletedAt != "" || s.ExpiresAt != "" || s.NextSyncAt != "" || s.Report != nil || s.Reason != "" {
				return false
			}
			continue
		}
		if s.Status != "complete" && s.Status != "failed" || s.StartedAt == "" || s.CompletedAt == "" || parse(s.CompletedAt).Before(parse(s.StartedAt)) || s.Enabled != (s.NextSyncAt != "") {
			return false
		}
		if s.NextSyncAt != "" && parse(s.NextSyncAt).Before(parse(s.CompletedAt)) {
			return false
		}
		if s.Status == "failed" {
			if !jobReason(s.Reason) || s.Report != nil || s.ExpiresAt != "" {
				return false
			}
			continue
		}
		if s.Reason != "" || s.ExpiresAt == "" || parse(s.ExpiresAt).After(parse(s.CompletedAt).Add(successTTL)) {
			return false
		}
		if s.Report != nil && (!ValidReport(*s.Report, s.Site, parse(s.CompletedAt)) || parse(s.Report.ObservedAt).Before(parse(s.StartedAt))) {
			return false
		}
	}
	return true
}
