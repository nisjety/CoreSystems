package http

import (
	"encoding/json"
	"errors"
	"mime"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type monitoringSource interface{ Monitoring() *siteaudit.Monitor }

func registerMonitoring(mux *http.ServeMux, key string, source PulseGetter) {
	provider, ok := source.(monitoringSource)
	if !ok || provider.Monitoring() == nil {
		return
	}
	mux.HandleFunc("/api/v1/marketing/monitoring", requireInternalKey(key, workLimiter(monitoringHandler(provider.Monitoring()))))
}
func monitoringHandler(service *siteaudit.Monitor) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var id, site, cadence string
		var websiteRevision, scheduleRevision int64
		var enabled bool
		switch r.Method {
		case http.MethodGet:
			q, err := strictQuery(r)
			if err != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 {
				monitoringError(w, 400)
				return
			}
			id, site = q.Get("websiteId"), q.Get("site")
		case http.MethodPatch:
			media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if err != nil || media != "application/json" {
				monitoringError(w, 415)
				return
			}
			fields, err := websiteObject(r, "websiteId", "site", "websiteRevision", "scheduleRevision", "enabled", "cadence")
			if err != nil {
				monitoringError(w, 400)
				return
			}
			var a, b, c bool
			id, a = objectString(fields, "websiteId")
			site, b = objectString(fields, "site")
			cadence, c = objectString(fields, "cadence")
			if !a || !b || !c || json.Unmarshal(fields["websiteRevision"], &websiteRevision) != nil || websiteRevision < 1 || websiteRevision > 1<<53-1 || string(fields["scheduleRevision"]) == "null" || json.Unmarshal(fields["scheduleRevision"], &scheduleRevision) != nil || scheduleRevision < 0 || scheduleRevision > 1<<53-1 || string(fields["enabled"]) != "true" && string(fields["enabled"]) != "false" {
				monitoringError(w, 400)
				return
			}
			enabled = string(fields["enabled"]) == "true"
		default:
			w.Header().Set("Allow", "GET, PATCH")
			monitoringError(w, 405)
			return
		}
		var value siteaudit.MonitorSnapshot
		var err error
		if r.Method == http.MethodGet {
			value, err = service.Get(id, site)
		} else {
			value, err = service.Update(id, site, websiteRevision, scheduleRevision, enabled, cadence)
		}
		if err != nil {
			status := 503
			switch {
			case errors.Is(err, siteaudit.ErrInvalid) || errors.Is(err, websites.ErrInvalid):
				status = 400
			case errors.Is(err, siteaudit.ErrConflict) || errors.Is(err, websites.ErrConflict):
				status = 409
			case errors.Is(err, siteaudit.ErrNotFound) || errors.Is(err, websites.ErrNotFound):
				status = 404
			case errors.Is(err, siteaudit.ErrCapacity):
				status = 429
				w.Header().Set("Retry-After", "60")
			}
			monitoringError(w, status)
			return
		}
		writeJSON(w, 200, map[string]any{"data": value})
	}
}
func monitoringError(w http.ResponseWriter, status int) {
	code, message := "monitoring_unavailable", "Website monitoring is unavailable; reload to confirm saved state"
	switch status {
	case 400:
		code, message = "invalid_monitoring_request", "Choose a registered website and daily or weekly cadence"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "monitoring_conflict", "Website or schedule changed; reload before saving"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	case 405:
		code, message = "method_not_allowed", "GET or PATCH is required"
	case 429:
		code, message = "rate_limited", "Wait for the current audit before changing monitoring"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
