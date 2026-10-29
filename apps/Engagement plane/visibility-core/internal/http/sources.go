package http

import (
	"context"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type sourceItem struct {
	Key         string `json:"key"`
	Implemented bool   `json:"implemented"`
	Mode        string `json:"mode"`
	Status      string `json:"status"`
	Refresh     string `json:"refresh"`
}

// This is a capability/access catalogue, not a provider-health check. Fresh
// Integration metadata can confirm an existing connection without fetching
// analytics. A pending project ID is never interpreted as a live grant.
func registerSources(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(websiteSource)
	if !ok || owner.WebsiteRegistry() == nil {
		return
	}
	registry := owner.WebsiteRegistry()
	mux.HandleFunc("/api/v1/marketing/sources", requireInternalKey(key, workLimiter(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", "GET")
			websiteError(w, 405)
			return
		}
		q, err := strictQuery(r)
		if err != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || !websiteIDPattern.MatchString(q.Get("websiteId")) {
			websiteError(w, 400)
			return
		}
		record, err := registry.Resolve(q.Get("websiteId"), q.Get("site"))
		if err != nil {
			websiteServiceError(w, err)
			return
		}
		if q.Get("site") == "" {
			websiteError(w, 400)
			return
		}
		site := record.Website
		items := sourceItems(site)
		if authority, ok := source.(interface {
			SourceStatuses(context.Context, websites.Website) map[string]string
		}); ok {
			statuses := authority.SourceStatuses(r.Context(), site)
			for i := range items {
				if status, exists := statuses[items[i].Key]; exists {
					items[i].Status = status
				}
			}
		}
		current, err := registry.Resolve(site.ID, site.Site)
		if err != nil || current.Website.Revision != site.Revision {
			websiteError(w, 409)
			return
		}
		writeJSON(w, 200, map[string]any{"data": map[string]any{"websiteId": site.ID, "site": site.Site, "websiteRevision": site.Revision, "source": "marketing_source_catalog", "items": items}})
	})))
}

func sourceItems(site websites.Website) []sourceItem {
	return []sourceItem{
		{Key: "quarry", Implemented: true, Mode: "public_check", Status: "available", Refresh: "preparation_job"},
		{Key: "pagespeed", Implemented: true, Mode: "public_check", Status: "available", Refresh: "preparation_job"},
		{Key: "search_console", Implemented: true, Mode: "oauth", Status: catalogAccess(site.Integrations.SearchConsole.Status), Refresh: "snapshot_read"},
		{Key: "ga4", Implemented: true, Mode: "oauth", Status: catalogAccess(site.Integrations.GA4.Status), Refresh: "snapshot_read"},
		{Key: "clarity", Implemented: true, Mode: "project_token", Status: catalogAccess(site.Integrations.Clarity.Status), Refresh: "snapshot_read"},
		{Key: "google_ads", Mode: "oauth", Status: "not_implemented", Refresh: "not_available"},
		{Key: "bing_webmaster", Mode: "oauth", Status: "not_implemented", Refresh: "not_available"},
		{Key: "bing_ai", Mode: "import", Status: "not_implemented", Refresh: "not_available"},
		{Key: "crux", Implemented: true, Mode: "public_check", Status: "available", Refresh: "public_signal_job"},
		{Key: "web_vitals", Implemented: true, Mode: "installation", Status: "not_connected", Refresh: "manual"},
		{Key: "cookiebot", Implemented: true, Mode: "api_key", Status: "not_connected", Refresh: "manual"},
		{Key: "algolia", Implemented: true, Mode: "oauth", Status: "not_connected", Refresh: "snapshot_read"},
		{Key: "google_trends", Implemented: true, Mode: "public_check", Status: "available", Refresh: "trends_job"},
		{Key: "common_crawl", Implemented: true, Mode: "public_archive", Status: "available", Refresh: "public_signal_job"},
	}
}
func catalogAccess(status string) string {
	switch status {
	case "connected", "needs_connection", "requires_consent", "needs_selection", "not_found", "source_unavailable", "pending_verification", "not_connected":
		return status
	case "unmapped":
		return "needs_connection"
	default:
		return "source_unavailable"
	}
}
