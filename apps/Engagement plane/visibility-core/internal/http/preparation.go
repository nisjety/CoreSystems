package http

import (
	"encoding/json"
	"errors"
	"log"
	"mime"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/preparation"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type preparationSource interface{ Preparation() *preparation.Service }

func preparationFor(source PulseGetter) *preparation.Service {
	if owner, ok := source.(preparationSource); ok {
		return owner.Preparation()
	}
	return nil
}
func registerPreparation(mux *http.ServeMux, key string, source PulseGetter) {
	jobs := preparationFor(source)
	if jobs == nil {
		return
	}
	mux.HandleFunc("/api/v1/marketing/preparation", requireInternalKey(key, workLimiter(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var id, site string
		var revision int64
		switch r.Method {
		case http.MethodGet:
			q, err := strictQuery(r)
			if err != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 {
				preparationError(w, 400)
				return
			}
			id, site = q.Get("websiteId"), q.Get("site")
		case http.MethodPost:
			media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if err != nil || media != "application/json" {
				preparationError(w, 415)
				return
			}
			fields, err := websiteObject(r, "websiteId", "site", "websiteRevision")
			if err != nil {
				preparationError(w, 400)
				return
			}
			var a, b bool
			id, a = objectString(fields, "websiteId")
			site, b = objectString(fields, "site")
			if !a || !b || json.Unmarshal(fields["websiteRevision"], &revision) != nil || revision < 1 {
				preparationError(w, 400)
				return
			}
		default:
			w.Header().Set("Allow", "GET, POST")
			preparationError(w, 405)
			return
		}
		var value preparation.Snapshot
		var err error
		status := 200
		if r.Method == http.MethodPost {
			value, err = jobs.Queue(id, site, revision)
			status = 202
		} else {
			value, err = jobs.Get(id, site)
		}
		if err != nil {
			preparationServiceError(w, err)
			return
		}
		writeJSON(w, status, map[string]any{"data": value})
	})))
}
func preparationServiceError(w http.ResponseWriter, err error) {
	status := 503
	switch {
	case errors.Is(err, preparation.ErrInvalid), errors.Is(err, websites.ErrInvalid):
		status = 400
	case errors.Is(err, websites.ErrNotFound):
		status = 404
	case errors.Is(err, preparation.ErrConflict), errors.Is(err, websites.ErrConflict):
		status = 409
	case errors.Is(err, preparation.ErrCapacity), errors.Is(err, preparation.ErrCooldown):
		status = 429
		w.Header().Set("Retry-After", "60")
	}
	preparationError(w, status)
}
func preparationError(w http.ResponseWriter, status int) {
	code, message := "preparation_unavailable", "Website preparation is unavailable; refresh its status or retry"
	switch status {
	case 400:
		code, message = "invalid_preparation_request", "Choose a registered website and its current revision"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before preparing"
	case 429:
		code, message = "preparation_busy", "Wait before retrying website preparation"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	case 405:
		code, message = "method_not_allowed", "GET or POST is required"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
func queueWebsitePreparation(jobs *preparation.Service, website websites.Website) {
	if jobs == nil {
		return
	}
	if _, err := jobs.Queue(website.ID, website.Site, website.Revision); err != nil {
		// Website persistence already succeeded. Never ask the caller to replay
		// that mutation: GET shows idle and the explicit preparation retry works.
		log.Printf("marketing-core: website saved but public preparation was not queued; retry preparation")
	}
}
