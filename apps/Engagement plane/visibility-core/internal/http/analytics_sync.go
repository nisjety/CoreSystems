package http

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"mime"
	"net/http"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type analyticsSyncSource interface{ AnalyticsSync() *analyticssync.Service }

func analyticsSyncFor(source PulseGetter) *analyticssync.Service {
	if owner, ok := source.(analyticsSyncSource); ok {
		return owner.AnalyticsSync()
	}
	return nil
}
func registerAnalyticsSync(mux *http.ServeMux, key string, source PulseGetter) {
	jobs := analyticsSyncFor(source)
	if jobs == nil {
		return
	}
	mux.HandleFunc("/api/v1/marketing/analytics-sync", requireInternalKey(key, workLimiter(analyticsSyncHandler(jobs))))
}
func analyticsSyncHandler(jobs *analyticssync.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var id, site, quarter string
		var revision int64
		var enabled bool
		switch r.Method {
		case http.MethodGet:
			q, err := strictQuery(r)
			if err != nil || len(q) != 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["quarter"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
				analyticsSyncError(w, 400)
				return
			}
			id, site, quarter = q.Get("websiteId"), q.Get("site"), q.Get("quarter")
		case http.MethodPost, http.MethodPatch:
			media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if err != nil || media != "application/json" {
				analyticsSyncError(w, 415)
				return
			}
			fields := []string{"websiteId", "site", "websiteRevision", "quarter"}
			if r.Method == http.MethodPatch {
				fields = append(fields, "enabled")
			}
			body, err := websiteObject(r, fields...)
			if err != nil {
				analyticsSyncError(w, 400)
				return
			}
			var a, b, c bool
			id, a = objectString(body, "websiteId")
			site, b = objectString(body, "site")
			quarter, c = objectString(body, "quarter")
			if !a || !b || !c || json.Unmarshal(body["websiteRevision"], &revision) != nil || revision < 1 {
				analyticsSyncError(w, 400)
				return
			}
			if r.Method == http.MethodPatch {
				raw := bytes.TrimSpace(body["enabled"])
				if !bytes.Equal(raw, []byte("true")) && !bytes.Equal(raw, []byte("false")) {
					analyticsSyncError(w, 400)
					return
				}
				_ = json.Unmarshal(raw, &enabled)
			}
		default:
			w.Header().Set("Allow", "GET, POST, PATCH")
			analyticsSyncError(w, 405)
			return
		}
		var value analyticssync.State
		var err error
		status := 200
		switch r.Method {
		case http.MethodPost:
			value, err = jobs.Queue(id, site, revision, quarter)
			status = 202
		case http.MethodPatch:
			value, err = jobs.SetEnabled(id, site, revision, quarter, enabled)
		default:
			value, err = jobs.Get(id, site, quarter)
		}
		if err != nil {
			analyticsSyncServiceError(w, err)
			return
		}
		writeJSON(w, status, map[string]any{"data": value})
	}
}
func analyticsSyncServiceError(w http.ResponseWriter, err error) {
	status := 503
	switch {
	case errors.Is(err, analyticssync.ErrInvalid), errors.Is(err, websites.ErrInvalid):
		status = 400
	case errors.Is(err, websites.ErrNotFound):
		status = 404
	case errors.Is(err, analyticssync.ErrConflict), errors.Is(err, websites.ErrConflict):
		status = 409
	case errors.Is(err, analyticssync.ErrCooldown), errors.Is(err, analyticssync.ErrCapacity):
		status = 429
		w.Header().Set("Retry-After", "60")
	}
	analyticsSyncError(w, status)
}
func analyticsSyncError(w http.ResponseWriter, status int) {
	code, message := "analytics_sync_unavailable", "Analytics synchronization is unavailable; retry later"
	switch status {
	case 400:
		code, message = "invalid_analytics_sync_request", "Choose one registered website, current revision and current-year quarter"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before synchronizing"
	case 429:
		code, message = "analytics_sync_busy", "Wait before retrying analytics synchronization"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	case 405:
		code, message = "method_not_allowed", "GET, POST or PATCH is required"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
func queueWebsiteAnalytics(jobs *analyticssync.Service, website websites.Website) {
	if jobs == nil {
		return
	}
	q := fmt.Sprintf("Q%d", (int(time.Now().UTC().Month())-1)/3+1)
	if _, err := jobs.Queue(website.ID, website.Site, website.Revision, q); err != nil {
		log.Printf("marketing-core: website saved but analytics synchronization was not queued; retry synchronization")
	}
}
