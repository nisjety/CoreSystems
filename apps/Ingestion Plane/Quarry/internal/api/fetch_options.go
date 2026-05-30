package api

import (
	"encoding/json"
	"strings"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/quarry/internal/config"
)

type proxyRequest struct {
	URL    string `json:"url,omitempty"`
	Type   string `json:"type,omitempty"`
	Region string `json:"region,omitempty"`
}

var fetchAliasKeys = []string{
	"formats",
	"headers",
	"waitFor",
	"onlyMainContent",
	"includeTags",
	"excludeTags",
	"actions",
	"mobile",
	"viewport",
	"location",
	"blockAds",
	"maxAge",
	"parserMode",
	"proxy",
}

func mergeTopLevelFetchAliases(body []byte) ([]byte, error) {
	if len(body) == 0 {
		return body, nil
	}

	var envelope map[string]json.RawMessage
	if err := json.Unmarshal(body, &envelope); err != nil {
		return nil, err
	}

	scrapeOptions := make(map[string]interface{})
	if raw, ok := envelope["scrapeOptions"]; ok && len(raw) > 0 {
		_ = json.Unmarshal(raw, &scrapeOptions)
	}

	merged := false
	for _, key := range fetchAliasKeys {
		raw, ok := envelope[key]
		if !ok || len(raw) == 0 {
			continue
		}
		var decoded interface{}
		if err := json.Unmarshal(raw, &decoded); err != nil {
			continue
		}
		scrapeOptions[key] = decoded
		merged = true
	}
	if !merged && len(scrapeOptions) == 0 {
		return body, nil
	}

	encodedScrapeOptions, err := json.Marshal(scrapeOptions)
	if err != nil {
		return nil, err
	}
	envelope["scrapeOptions"] = encodedScrapeOptions
	return json.Marshal(envelope)
}

func fieldPresence(body []byte) map[string]struct{} {
	if len(body) == 0 {
		return nil
	}
	var envelope map[string]json.RawMessage
	if err := json.Unmarshal(body, &envelope); err != nil {
		return nil
	}
	present := make(map[string]struct{}, len(envelope))
	for key := range envelope {
		present[key] = struct{}{}
	}
	return present
}

func (h *Handler) resolveProxyURL(c *fiber.Ctx, raw *proxyRequest) string {
	if raw != nil {
		if direct := strings.TrimSpace(raw.URL); direct != "" {
			return direct
		}
		selected := selectProxyFromPool(h.cfg, raw.Region, raw.Type)
		if selected != "" {
			return selected
		}
	}
	if c == nil {
		return ""
	}
	proxyURL, _ := c.Locals("proxy_url").(string)
	return strings.TrimSpace(proxyURL)
}

func selectProxyFromPool(cfg *config.Config, region, proxyType string) string {
	if cfg == nil {
		return ""
	}
	desiredRegion := strings.ToLower(strings.TrimSpace(region))
	desiredType := strings.ToLower(strings.TrimSpace(proxyType))
	for _, entry := range cfg.ProxyPool {
		parts := strings.SplitN(strings.TrimSpace(entry), "|", 3)
		if len(parts) == 0 || strings.TrimSpace(parts[0]) == "" {
			continue
		}
		entryURL := strings.TrimSpace(parts[0])
		entryType := ""
		entryRegion := ""
		if len(parts) >= 2 {
			entryType = strings.ToLower(strings.TrimSpace(parts[1]))
		}
		if len(parts) >= 3 {
			entryRegion = strings.ToLower(strings.TrimSpace(parts[2]))
		}
		if desiredType != "" && desiredType != entryType {
			continue
		}
		if desiredRegion != "" && desiredRegion != entryRegion {
			continue
		}
		return entryURL
	}
	return ""
}

func extractFetchMetadata(raw interface{}) (int, string) {
	payload, ok := raw.(map[string]interface{})
	if !ok {
		return 0, ""
	}
	status := intFromAny(payload["status"])
	contentType := stringFromAny(payload["contentType"], "")
	return status, contentType
}

func stringSliceFromAny(raw interface{}) []string {
	switch typed := raw.(type) {
	case []string:
		return append([]string(nil), typed...)
	case []interface{}:
		out := make([]string, 0, len(typed))
		for _, item := range typed {
			if value, ok := item.(string); ok && strings.TrimSpace(value) != "" {
				out = append(out, strings.TrimSpace(value))
			}
		}
		return out
	default:
		return nil
	}
}
