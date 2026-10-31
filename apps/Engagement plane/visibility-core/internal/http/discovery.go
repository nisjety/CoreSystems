package http

import (
	"encoding/json"
	"errors"
	"mime"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/discovery"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type discoverySource interface{ Discovery() *discovery.Service }

func registerDiscovery(mux *http.ServeMux, key string, source PulseGetter) {
	provider, ok := source.(discoverySource)
	if !ok || provider.Discovery() == nil {
		return
	}
	mux.HandleFunc("/api/v1/marketing/discovery", requireInternalKey(key, workLimiter(discoveryHandler(provider.Discovery()))))
}
func discoveryHandler(service *discovery.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var id, site string
		var revision int64
		switch r.Method {
		case http.MethodGet:
			q, err := strictQuery(r)
			if err != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 {
				discoveryError(w, 400)
				return
			}
			id, site = q.Get("websiteId"), q.Get("site")
		case http.MethodPost:
			media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if err != nil || media != "application/json" {
				discoveryError(w, 415)
				return
			}
			fields, err := websiteObject(r, "websiteId", "site", "websiteRevision")
			if err != nil {
				discoveryError(w, 400)
				return
			}
			var a, b bool
			id, a = objectString(fields, "websiteId")
			site, b = objectString(fields, "site")
			if !a || !b || json.Unmarshal(fields["websiteRevision"], &revision) != nil || revision < 1 {
				discoveryError(w, 400)
				return
			}
		default:
			w.Header().Set("Allow", "GET, POST")
			discoveryError(w, 405)
			return
		}
		var value discovery.Snapshot
		var err error
		if r.Method == http.MethodPost {
			value, err = service.Start(id, site, revision)
		} else {
			value, err = service.Get(id, site)
		}
		if err != nil {
			switch {
			case errors.Is(err, websites.ErrInvalid):
				discoveryError(w, 400)
			case errors.Is(err, websites.ErrNotFound):
				discoveryError(w, 404)
			case errors.Is(err, websites.ErrConflict):
				discoveryError(w, 409)
			case errors.Is(err, discovery.ErrCooldown):
				w.Header().Set("Retry-After", "60")
				discoveryError(w, 429)
			default:
				discoveryError(w, 503)
			}
			return
		}
		writeJSON(w, 200, map[string]any{"data": value})
	}
}
func discoveryError(w http.ResponseWriter, status int) {
	code, message := "discovery_unavailable", "Page discovery is unavailable"
	switch status {
	case 400:
		code, message = "invalid_discovery_request", "Choose a registered website"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before discovering pages"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	case 405:
		code, message = "method_not_allowed", "GET or POST is required"
	case 429:
		code, message = "rate_limited", "Wait before starting another discovery"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
