package discovery

import (
	"net/url"
	"path"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

// SafeURL is a display/scope filter, not SSRF protection. Quarry independently
// validates and pins every network destination. Query-bearing/private routes
// are excluded from this public URL inventory rather than redacted into aliases.
func SafeURL(site, raw string) bool {
	canonical, err := pulse.CanonicalSite(site)
	if err != nil || canonical != site || len(raw) > 2048 || !utf8.ValidString(raw) {
		return false
	}
	u, err := url.Parse(raw)
	if err != nil || u.String() != raw || u.Scheme != "https" || u.User != nil || u.Opaque != "" || u.RawQuery != "" || u.ForceQuery || u.Fragment != "" || strings.Contains(raw, "#") {
		return false
	}
	base, _ := url.Parse(site)
	root := strings.TrimPrefix(base.Hostname(), "www.")
	if u.Port() != "" || (u.Hostname() != root && u.Hostname() != "www."+root) || u.Host != u.Hostname() || u.Path == "" || !strings.HasPrefix(u.Path, "/") {
		return false
	}
	decoded := u.Path
	for i := 0; i < 3; i++ {
		if strings.ContainsAny(decoded, "\\") || strings.IndexFunc(decoded, unicode.IsControl) >= 0 {
			return false
		}
		next, err := url.PathUnescape(decoded)
		if err != nil {
			return false
		}
		if next == decoded {
			break
		}
		decoded = next
	}
	if strings.Contains(decoded, "%") || path.Clean(decoded) != strings.TrimSuffix(decoded, "/") && decoded != "/" {
		return false
	}
	for _, segment := range strings.Split(strings.ToLower(decoded), "/") {
		switch segment {
		case "admin", "api", "oauth", "auth", "login", "signin", "sign-in", "callback", "account", "dashboard", "wp-admin", "wp-login.php", "checkout", "reset-password", "unsubscribe":
			return false
		}
	}
	return true
}
func sanitizePages(site string, pages []Page) []Page {
	out := []Page{}
	seen := map[string]bool{}
	for _, p := range pages {
		if !SafeURL(site, p.URL) || seen[p.URL] {
			continue
		}
		title := strings.TrimSpace(p.Title)
		if !utf8.ValidString(title) || len(title) > 200 || strings.IndexFunc(title, unicode.IsControl) >= 0 {
			title = ""
		}
		out = append(out, Page{URL: p.URL, Title: title})
		seen[p.URL] = true
		if len(out) == Limit {
			break
		}
	}
	return out
}
