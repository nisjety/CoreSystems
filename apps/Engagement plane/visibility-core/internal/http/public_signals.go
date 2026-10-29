package http

import (
	"bytes"
	"encoding/json"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/publicsignals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"log"
	"mime"
	"net/http"
)

type publicSignalsSource interface{ PublicSignals() *publicsignals.Service }

func publicSignalsFor(source PulseGetter) *publicsignals.Service {
	if owner, ok := source.(publicSignalsSource); ok {
		return owner.PublicSignals()
	}
	return nil
}
func registerPublicSignals(mux *http.ServeMux, key string, source PulseGetter) {
	jobs := publicSignalsFor(source)
	if jobs == nil {
		return
	}
	mux.HandleFunc("/api/v1/marketing/public-signals", requireInternalKey(key, workLimiter(publicSignalsHandler(jobs))))
}
func publicSignalsHandler(jobs *publicsignals.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var id, site string
		var revision int64
		var enabled bool
		switch r.Method {
		case http.MethodGet:
			q, err := strictQuery(r)
			if err != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
				publicSignalsError(w, 400)
				return
			}
			id, site = q.Get("websiteId"), q.Get("site")
		case http.MethodPost, http.MethodPatch:
			media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if err != nil || media != "application/json" {
				publicSignalsError(w, 415)
				return
			}
			fields := []string{"websiteId", "site", "websiteRevision"}
			if r.Method == http.MethodPatch {
				fields = append(fields, "enabled")
			}
			body, err := websiteObject(r, fields...)
			if err != nil {
				publicSignalsError(w, 400)
				return
			}
			var a, b bool
			id, a = objectString(body, "websiteId")
			site, b = objectString(body, "site")
			if !a || !b || json.Unmarshal(body["websiteRevision"], &revision) != nil || revision < 1 {
				publicSignalsError(w, 400)
				return
			}
			if r.Method == http.MethodPatch {
				raw := bytes.TrimSpace(body["enabled"])
				if !bytes.Equal(raw, []byte("true")) && !bytes.Equal(raw, []byte("false")) {
					publicSignalsError(w, 400)
					return
				}
				_ = json.Unmarshal(raw, &enabled)
			}
		default:
			w.Header().Set("Allow", "GET, POST, PATCH")
			publicSignalsError(w, 405)
			return
		}
		var value publicsignals.State
		var err error
		status := 200
		switch r.Method {
		case http.MethodPost:
			value, err = jobs.Queue(id, site, revision)
			status = 202
		case http.MethodPatch:
			value, err = jobs.SetEnabled(id, site, revision, enabled)
		default:
			value, err = jobs.Get(id, site)
		}
		if err != nil {
			publicSignalsServiceError(w, err)
			return
		}
		writeJSON(w, status, map[string]any{"data": value})
	}
}
func publicSignalsServiceError(w http.ResponseWriter, err error) {
	status := 503
	switch {
	case errors.Is(err, publicsignals.ErrInvalid), errors.Is(err, websites.ErrInvalid):
		status = 400
	case errors.Is(err, websites.ErrNotFound):
		status = 404
	case errors.Is(err, publicsignals.ErrConflict), errors.Is(err, websites.ErrConflict):
		status = 409
	case errors.Is(err, publicsignals.ErrCooldown), errors.Is(err, publicsignals.ErrCapacity):
		status = 429
		w.Header().Set("Retry-After", "60")
	}
	publicSignalsError(w, status)
}
func publicSignalsError(w http.ResponseWriter, status int) {
	code, message := "public_signals_unavailable", "Public signals are unavailable; retry later"
	switch status {
	case 400:
		code, message = "invalid_public_signals_request", "Choose a registered website and its current revision"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before checking"
	case 429:
		code, message = "public_signals_busy", "Wait before retrying public signals"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	case 405:
		code, message = "method_not_allowed", "GET, POST or PATCH is required"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
func queueWebsitePublic(jobs *publicsignals.Service, website websites.Website) {
	if jobs == nil {
		return
	}
	if _, err := jobs.Queue(website.ID, website.Site, website.Revision); err != nil {
		log.Printf("marketing-core: website saved but public signals were not queued; retry public signals")
	}
}
