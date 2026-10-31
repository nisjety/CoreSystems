package http

import (
	"encoding/json"
	"errors"
	"net/http"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

type workSource interface{ WorkQueue() *workqueue.Service }

var workIDPattern = regexp.MustCompile(`^task_[a-f0-9]{32}$`)

func registerWork(mux *http.ServeMux, key string, source PulseGetter) {
	provider, ok := source.(workSource)
	if !ok || provider.WorkQueue() == nil {
		return
	}
	handler := requireInternalKey(key, workLimiter(workHandler(provider.WorkQueue())))
	mux.HandleFunc("/api/v1/marketing/work", handler)
	mux.HandleFunc("/api/v1/marketing/work/", handler)
}
func workHandler(service *workqueue.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		const base = "/api/v1/marketing/work"
		if strings.HasPrefix(r.URL.Path, base+"/") && strings.HasSuffix(r.URL.Path, "/planning") {
			id := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, base+"/"), "/planning")
			if !workIDPattern.MatchString(id) {
				workError(w, 404, "work_not_found")
				return
			}
			planningWorkRequest(w, r, service, id)
			return
		}
		if r.URL.Path == base+"/review" {
			reviewWorkRequest(w, r, service)
			return
		}
		if r.URL.Path == base+"/audit" {
			auditWorkRequest(w, r, service)
			return
		}
		if r.URL.Path == base+"/technical" {
			technicalWorkRequest(w, r, service)
			return
		}
		if strings.HasPrefix(r.URL.Path, base+"/") && strings.HasSuffix(r.URL.Path, "/verify") {
			id := strings.TrimSuffix(strings.TrimPrefix(r.URL.Path, base+"/"), "/verify")
			if !workIDPattern.MatchString(id) {
				workError(w, 404, "work_not_found")
				return
			}
			verifyWorkRequest(w, r, service, id)
			return
		}
		if r.URL.Path == base && r.Method == http.MethodGet {
			query, err := strictQuery(r)
			if err != nil || len(query) != 2 || len(query["websiteId"]) != 1 || len(query["site"]) != 1 {
				workError(w, 400, "work_invalid_request")
				return
			}
			value, err := service.List(query.Get("websiteId"), query.Get("site"))
			workResult(w, value, err, 200)
			return
		}
		id := strings.TrimPrefix(r.URL.Path, base+"/")
		if r.URL.Path != base && (!workIDPattern.MatchString(id) || strings.Contains(id, "/")) {
			workError(w, 404, "work_not_found")
			return
		}
		if (r.URL.Path == base && r.Method != http.MethodPost) || (r.URL.Path != base && r.Method != http.MethodPatch) {
			if r.URL.Path == base {
				w.Header().Set("Allow", "GET, POST")
			} else {
				w.Header().Set("Allow", "PATCH")
			}
			workError(w, 405, "method_not_allowed")
			return
		}
		fields := []string{"websiteId", "site", "websiteRevision", "quarter", "query"}
		if r.Method == http.MethodPatch {
			fields = []string{"websiteId", "site", "websiteRevision", "revision", "status"}
		}
		body, err := websiteObject(r, fields...)
		if err != nil {
			workError(w, 400, "work_invalid_request")
			return
		}
		websiteID, a := objectString(body, "websiteId")
		site, b := objectString(body, "site")
		var websiteRevision int64
		if err := json.Unmarshal(body["websiteRevision"], &websiteRevision); err != nil || !a || !b || websiteRevision < 1 {
			workError(w, 400, "work_invalid_request")
			return
		}
		if r.Method == http.MethodPost {
			quarter, a := objectString(body, "quarter")
			key, b := objectString(body, "query")
			if !a || !b {
				workError(w, 400, "work_invalid_request")
				return
			}
			value, created, err := service.Save(r.Context(), websiteID, site, websiteRevision, quarter, key)
			status := 200
			if created {
				status = 201
			}
			workResult(w, value, err, status)
			return
		}
		status, ok := objectString(body, "status")
		revision := objectRevision(body)
		if !ok || revision < 1 {
			workError(w, 400, "work_invalid_request")
			return
		}
		value, err := service.Change(websiteID, site, websiteRevision, id, revision, status)
		workResult(w, value, err, 200)
	}
}
func workResult(w http.ResponseWriter, value workqueue.Snapshot, err error, status int) {
	if err == nil {
		writeJSON(w, status, map[string]any{"data": value})
		return
	}
	switch {
	case errors.Is(err, workqueue.ErrInvalid):
		workError(w, 400, "work_invalid_request")
	case errors.Is(err, workqueue.ErrNotFound):
		workError(w, 404, "work_not_found")
	case errors.Is(err, workqueue.ErrConflict):
		workError(w, 409, "work_conflict")
	case errors.Is(err, workqueue.ErrSourceUnavailable):
		workError(w, 503, "work_source_unavailable")
	case errors.Is(err, workqueue.ErrCapacity):
		workError(w, 409, "work_capacity")
	default:
		workError(w, 503, "work_unavailable")
	}
}
func workError(w http.ResponseWriter, status int, code string) {
	message := "Marketing work is unavailable"
	switch status {
	case 400:
		message = "Choose a valid website and work request"
	case 404:
		message = "Website, finding, or task was not found"
	case 409:
		message = "Work or website changed; reload before saving"
	case 405:
		message = "Method is not supported"
	case 429:
		message = "Too many marketing work requests"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
func workLimiter(next http.HandlerFunc) http.HandlerFunc {
	var mu sync.Mutex
	var until time.Time
	reads, writes := 0, 0
	return func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		if !time.Now().Before(until) {
			until = time.Now().Add(time.Minute)
			reads, writes = 0, 0
		}
		blocked := false
		if r.Method == http.MethodGet {
			reads++
			blocked = reads > 120
		} else {
			writes++
			blocked = writes > 30
		}
		mu.Unlock()
		if blocked {
			w.Header().Set("Cache-Control", "no-store")
			workError(w, 429, "rate_limited")
			return
		}
		next(w, r)
	}
}
