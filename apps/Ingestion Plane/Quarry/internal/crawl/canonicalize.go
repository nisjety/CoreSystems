package crawl

import (
	"fmt"
	"net"
	"net/url"
	"path"
	"sort"
	"strings"

	"golang.org/x/net/publicsuffix"
)

func CanonicalizeURL(raw string, base *url.URL, ignoreQuery bool) (string, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		return "", fmt.Errorf("url is required")
	}
	if strings.HasPrefix(trimmed, "#") {
		return "", fmt.Errorf("fragment-only url is not supported")
	}

	parsed, err := url.Parse(trimmed)
	if err != nil {
		return "", fmt.Errorf("parse url: %w", err)
	}
	if base != nil {
		parsed = base.ResolveReference(parsed)
	}
	if parsed.Scheme != "http" && parsed.Scheme != "https" {
		return "", fmt.Errorf("unsupported url scheme")
	}
	if parsed.Host == "" {
		return "", fmt.Errorf("url host is required")
	}

	parsed.Fragment = ""
	parsed.User = nil
	parsed.Host = normalizeHost(parsed)
	parsed.Path = normalizePath(parsed.Path)

	if ignoreQuery {
		parsed.RawQuery = ""
	} else if parsed.RawQuery != "" {
		query := parsed.Query()
		keys := make([]string, 0, len(query))
		for key := range query {
			keys = append(keys, key)
		}
		sort.Strings(keys)

		rebuilt := url.Values{}
		for _, key := range keys {
			values := append([]string(nil), query[key]...)
			sort.Strings(values)
			for _, value := range values {
				rebuilt.Add(key, value)
			}
		}
		parsed.RawQuery = rebuilt.Encode()
	}

	return parsed.String(), nil
}

func normalizeHost(parsed *url.URL) string {
	host := strings.ToLower(strings.TrimSpace(parsed.Hostname()))
	if host == "" {
		return ""
	}

	port := parsed.Port()
	switch {
	case port == "":
		return host
	case parsed.Scheme == "http" && port == "80":
		return host
	case parsed.Scheme == "https" && port == "443":
		return host
	default:
		return net.JoinHostPort(host, port)
	}
}

func normalizePath(raw string) string {
	cleaned := path.Clean("/" + strings.TrimSpace(raw))
	if cleaned == "." || cleaned == "" {
		return "/"
	}
	if cleaned != "/" && strings.HasSuffix(raw, "/") {
		cleaned = strings.TrimSuffix(cleaned, "/")
	}
	if cleaned != "/" {
		cleaned = strings.TrimSuffix(cleaned, "/")
	}
	if cleaned == "" {
		return "/"
	}
	return cleaned
}

func BaseDomain(host string) string {
	trimmed := strings.ToLower(strings.TrimSpace(host))
	if trimmed == "" {
		return ""
	}
	base, err := publicsuffix.EffectiveTLDPlusOne(trimmed)
	if err != nil {
		return trimmed
	}
	return base
}
