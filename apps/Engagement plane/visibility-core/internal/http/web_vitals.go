package http

import (
	"encoding/json"
	"errors"
	"mime"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/webvitals"
)

type webVitalsSource interface{ WebVitals() *webvitals.Service }

func registerWebVitals(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(webVitalsSource)
	if !ok || owner.WebVitals() == nil {
		return
	}
	service := owner.WebVitals()
	mux.HandleFunc("/api/v1/marketing/web-vitals", requireInternalKey(key, workLimiter(webVitalsGet(service))))
	mux.HandleFunc("/api/v1/marketing/web-vitals/sync", requireInternalKey(key, workLimiter(webVitalsSync(service))))
}
func webVitalsGet(service *webvitals.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", "GET")
			webVitalsError(w, 405)
			return
		}
		q, err := strictQuery(r)
		if err != nil || len(q) != 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["environment"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
			webVitalsError(w, 400)
			return
		}
		data, err := service.Get(q.Get("websiteId"), q.Get("site"), q.Get("environment"))
		if err != nil {
			webVitalsServiceError(w, err)
			return
		}
		writeJSON(w, 200, map[string]any{"data": data})
	}
}
func webVitalsSync(service *webvitals.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodPost {
			w.Header().Set("Allow", "POST")
			webVitalsError(w, 405)
			return
		}
		media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
		if err != nil || media != "application/json" {
			webVitalsError(w, 415)
			return
		}
		body, err := websiteObject(r, "websiteId", "site", "websiteRevision", "environment")
		if err != nil {
			webVitalsError(w, 400)
			return
		}
		id, a := objectString(body, "websiteId")
		site, b := objectString(body, "site")
		env, c := objectString(body, "environment")
		var revision int64
		if !a || !b || !c || json.Unmarshal(body["websiteRevision"], &revision) != nil || revision < 1 {
			webVitalsError(w, 400)
			return
		}
		data, err := service.Sync(r.Context(), id, site, revision, env)
		if err != nil {
			webVitalsServiceError(w, err)
			return
		}
		writeJSON(w, 200, map[string]any{"data": data})
	}
}
func webVitalsServiceError(w http.ResponseWriter, err error) {
	status := 503
	switch {
	case errors.Is(err, webvitals.ErrUnavailable):
		status = 503
	case errors.Is(err, webvitals.ErrInvalid), errors.Is(err, websites.ErrInvalid):
		status = 400
	case errors.Is(err, webvitals.ErrForbidden):
		status = 403
	case errors.Is(err, websites.ErrNotFound):
		status = 404
	case errors.Is(err, websites.ErrConflict):
		status = 409
	case errors.Is(err, webvitals.ErrCooldown), errors.Is(err, webvitals.ErrCapacity):
		status = 429
		w.Header().Set("Retry-After", "60")
	}
	webVitalsError(w, status)
}
func webVitalsError(w http.ResponseWriter, status int) {
	code, message := "web_vitals_unavailable", "First-party performance measurements are unavailable; retry later"
	switch status {
	case 400:
		code, message = "invalid_web_vitals_request", "Choose a registered website and production or development environment"
	case 403:
		code, message = "provider_authorization_required", "Connect an authorized performance installation for this registered website"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before collecting"
	case 429:
		code, message = "web_vitals_busy", "Wait before refreshing first-party measurements"
	case 405:
		code, message = "method_not_allowed", "Use the permitted method"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
