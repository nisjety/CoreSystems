package pulse

import (
	"errors"
	"net"
	"net/url"
	"strings"
)

// CanonicalSite accepts a public HTTPS website origin, never a page URL or
// network destination. The value is only used to select a verified GSC
// property; Marketing Core does not request the submitted URL.
func CanonicalSite(input string) (string, error) {
	if len(input) == 0 || len(input) > 2048 || strings.ContainsAny(input, "?#\\\r\n\t ") {
		return "", errors.New("site must be a public HTTPS origin")
	}
	parsed, err := url.Parse(input)
	if err != nil || parsed.Scheme != "https" || parsed.Opaque != "" || parsed.User != nil || parsed.Host == "" || parsed.RawQuery != "" || parsed.Fragment != "" || parsed.ForceQuery {
		return "", errors.New("site must be a public HTTPS origin")
	}
	if parsed.Path != "" && parsed.Path != "/" || parsed.RawPath != "" || parsed.Host != parsed.Hostname() {
		return "", errors.New("site must be a public HTTPS origin")
	}
	host := strings.ToLower(parsed.Hostname())
	if !validPublicHostname(host) {
		return "", errors.New("site must be a public HTTPS origin")
	}
	return "https://" + host + "/", nil
}

// GA4ReportingSiteAllowed permits only the registered origin or its exact
// one-label www variant. It never authorizes arbitrary subdomains or parents.
func GA4ReportingSiteAllowed(site, reportingSite string) bool {
	canonical, err := CanonicalSite(site)
	if err != nil || canonical != site {
		return false
	}
	canonical, err = CanonicalSite(reportingSite)
	if err != nil || canonical != reportingSite {
		return false
	}
	if reportingSite == site {
		return true
	}
	host := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
	if strings.HasPrefix(host, "www.") {
		return reportingSite == "https://"+strings.TrimPrefix(host, "www.")+"/"
	}
	return reportingSite == "https://www."+host+"/"
}

func validPublicHostname(host string) bool {
	if len(host) > 253 || net.ParseIP(host) != nil || !strings.Contains(host, ".") ||
		host == "localhost" || strings.HasSuffix(host, ".localhost") ||
		strings.HasSuffix(host, ".local") || strings.HasSuffix(host, ".internal") {
		return false
	}
	for _, label := range strings.Split(host, ".") {
		if len(label) == 0 || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return false
		}
		for _, character := range label {
			if !((character >= 'a' && character <= 'z') || (character >= '0' && character <= '9') || character == '-') {
				return false
			}
		}
	}
	return true
}

func propertyMatchesSite(property, site string) (int, bool) {
	if strings.HasPrefix(property, "sc-domain:") {
		domain := strings.ToLower(strings.TrimPrefix(property, "sc-domain:"))
		if !validPublicHostname(domain) || property != "sc-domain:"+domain {
			return 0, false
		}
		host := strings.TrimSuffix(strings.TrimPrefix(site, "https://"), "/")
		// A parent domain property includes subdomains, but its date-only
		// analytics cannot isolate one selected subdomain. Require exact host.
		if host == domain || host == "www."+domain {
			return len(domain), true
		}
		return 0, false
	}
	canonical, err := CanonicalSite(property)
	if err != nil || canonical != property || !GA4ReportingSiteAllowed(site, canonical) {
		return 0, false
	}
	if canonical != site {
		return 5_000, true
	}
	return 10_000, true
}

func GSCPropertyForSiteAllowed(property, site string) bool {
	_, ok := propertyMatchesSite(property, site)
	return ok
}
