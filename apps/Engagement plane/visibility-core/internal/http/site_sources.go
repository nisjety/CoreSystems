package http

import (
	"encoding/json"
	"errors"
	"mime"
	"net/http"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type siteSourcesSource interface{ SiteSources() *sitesources.Service }

func registerSiteSources(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(siteSourcesSource)
	if !ok || owner.SiteSources() == nil {
		return
	}
	service := owner.SiteSources()
	mux.HandleFunc("/api/v1/marketing/site-sources", requireInternalKey(key, workLimiter(siteSourcesGet(service))))
	mux.HandleFunc("/api/v1/marketing/site-sources/sync", requireInternalKey(key, workLimiter(siteSourcesSync(service))))
	mux.HandleFunc("/api/v1/marketing/site-sources/collection", requireInternalKey(key, workLimiter(siteSourceCollection(service))))
}
func siteSourcesGet(service *sitesources.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != "GET" {
			w.Header().Set("Allow", "GET")
			siteSourcesError(w, 405)
			return
		}
		q, err := strictQuery(r)
		if err != nil || len(q) != 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["provider"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
			siteSourcesError(w, 400)
			return
		}
		data, err := service.GetContext(r.Context(), q.Get("websiteId"), q.Get("site"), q.Get("provider"))
		if err != nil {
			siteSourcesServiceError(w, err)
			return
		}
		writeJSON(w, 200, map[string]any{"data": data})
	}
}
func siteSourcesSync(service *sitesources.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != "POST" {
			w.Header().Set("Allow", "POST")
			siteSourcesError(w, 405)
			return
		}
		if err := http.NewResponseController(w).SetWriteDeadline(time.Now().Add(38 * time.Second)); err != nil && !errors.Is(err, http.ErrNotSupported) {
			siteSourcesError(w, 503)
			return
		}
		media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
		if err != nil || media != "application/json" {
			siteSourcesError(w, 415)
			return
		}
		body, err := websiteObject(r, "websiteId", "site", "websiteRevision", "provider")
		if err != nil {
			siteSourcesError(w, 400)
			return
		}
		id, a := objectString(body, "websiteId")
		site, b := objectString(body, "site")
		provider, c := objectString(body, "provider")
		var revision int64
		if !a || !b || !c || json.Unmarshal(body["websiteRevision"], &revision) != nil || revision < 1 {
			siteSourcesError(w, 400)
			return
		}
		data, err := service.Sync(r.Context(), id, site, revision, provider)
		if err != nil {
			siteSourcesServiceError(w, err)
			return
		}
		writeJSON(w, 200, map[string]any{"data": data})
	}
}
func siteSourcesServiceError(w http.ResponseWriter, err error) {
	status := 503
	switch {
	case errors.Is(err, sitesources.ErrInvalid), errors.Is(err, websites.ErrInvalid):
		status = 400
	case errors.Is(err, sitesources.ErrForbidden):
		status = 403
	case errors.Is(err, websites.ErrNotFound):
		status = 404
	case errors.Is(err, websites.ErrConflict):
		status = 409
	case errors.Is(err, sitesources.ErrCooldown), errors.Is(err, sitesources.ErrCapacity):
		status = 429
		w.Header().Set("Retry-After", "60")
	}
	siteSourcesError(w, status)
}
func siteSourcesError(w http.ResponseWriter, status int) {
	code, message := "site_sources_unavailable", "Read-only site source reporting is unavailable; retry later"
	switch status {
	case 400:
		code, message = "invalid_site_sources_request", "Choose a registered website and supported provider"
	case 403:
		code, message = "provider_authorization_required", "Connect an authorized provider account for this registered website"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before refreshing"
	case 429:
		code, message = "site_sources_busy", "Wait before refreshing this site source"
	case 405:
		code, message = "method_not_allowed", "Use the permitted method"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
