package middleware

import (
	"strings"
	"sync/atomic"

	"github.com/gofiber/fiber/v2"
)

// ProxyType describes the proxy behavior profile.
type ProxyType string

const (
	ProxyBasic    ProxyType = "basic"
	ProxyStealth  ProxyType = "stealth"
	ProxyEnhanced ProxyType = "enhanced"
	ProxyAuto     ProxyType = "auto" // selects based on target domain
)

type ProxyConfig struct {
	Enabled     bool
	Pool        []string
	DefaultType ProxyType
}

type ProxyEntry struct {
	URL    string
	Type   ProxyType
	Region string // e.g. "us", "eu", "ap"
}

func ProxyRotation(cfg ProxyConfig) fiber.Handler {
	if !cfg.Enabled || len(cfg.Pool) == 0 {
		return func(c *fiber.Ctx) error { return c.Next() }
	}

	entries := parseProxyPool(cfg.Pool)
	if len(entries) == 0 {
		return func(c *fiber.Ctx) error { return c.Next() }
	}

	var counter uint64

	return func(c *fiber.Ctx) error {
		// Allow per-request proxy override from header.
		requestedRegion := strings.ToLower(strings.TrimSpace(c.Get("X-Proxy-Region")))
		requestedType := ProxyType(strings.ToLower(strings.TrimSpace(c.Get("X-Proxy-Type"))))

		selected := selectProxy(entries, &counter, requestedRegion, requestedType)
		c.Locals("proxy_url", selected.URL)
		c.Locals("proxy_type", string(selected.Type))
		c.Locals("proxy_region", selected.Region)
		return c.Next()
	}
}

// selectProxy picks a proxy from the pool using round-robin, optionally
// filtering by region and type.
func selectProxy(entries []ProxyEntry, counter *uint64, region string, proxyType ProxyType) ProxyEntry {
	// If filters requested, try to match.
	if region != "" || proxyType != "" {
		var filtered []ProxyEntry
		for _, e := range entries {
			regionOK := region == "" || e.Region == region
			typeOK := proxyType == "" || e.Type == proxyType
			if regionOK && typeOK {
				filtered = append(filtered, e)
			}
		}
		if len(filtered) > 0 {
			idx := atomic.AddUint64(counter, 1)
			return filtered[(idx-1)%uint64(len(filtered))]
		}
	}

	// Fallback: round-robin across all proxies.
	idx := atomic.AddUint64(counter, 1)
	return entries[(idx-1)%uint64(len(entries))]
}

// parseProxyPool parses proxy strings in the format:
//   "http://proxy:8080"                     → basic, no region
//   "http://proxy:8080|stealth|us"          → stealth, us region
//   "http://proxy:8080|enhanced|eu"         → enhanced, eu region
func parseProxyPool(pool []string) []ProxyEntry {
	entries := make([]ProxyEntry, 0, len(pool))
	for _, raw := range pool {
		parts := strings.SplitN(raw, "|", 3)
		url := strings.TrimSpace(parts[0])
		if url == "" {
			continue
		}
		entry := ProxyEntry{URL: url, Type: ProxyBasic}
		if len(parts) >= 2 {
			t := ProxyType(strings.ToLower(strings.TrimSpace(parts[1])))
			if t == ProxyStealth || t == ProxyEnhanced || t == ProxyAuto {
				entry.Type = t
			}
		}
		if len(parts) >= 3 {
			entry.Region = strings.ToLower(strings.TrimSpace(parts[2]))
		}
		entries = append(entries, entry)
	}
	return entries
}
