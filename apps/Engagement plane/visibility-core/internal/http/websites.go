package http

import (
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/analyticssync"
	"github.com/triodelab/engagement-plane/visibility-core/internal/preparation"
	"github.com/triodelab/engagement-plane/visibility-core/internal/publicsignals"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type websiteSource interface{ WebsiteRegistry() *websites.Service }

func registerWebsites(mux *http.ServeMux, key string, source PulseGetter) {
	provider, ok := source.(websiteSource)
	if !ok || provider.WebsiteRegistry() == nil {
		return
	}
	registry := provider.WebsiteRegistry()
	handler := requireInternalKey(key, websiteLimiter(websiteHandlerWithSync(registry, preparationFor(source), analyticsSyncFor(source), publicSignalsFor(source))))
	mux.HandleFunc("/api/v1/marketing/websites", handler)
	mux.HandleFunc("/api/v1/marketing/websites/", handler)
}
func websiteHandler(registry *websites.Service, preparationServices ...*preparation.Service) http.HandlerFunc {
	var jobs *preparation.Service
	if len(preparationServices) > 0 {
		jobs = preparationServices[0]
	}
	return websiteHandlerWithSync(registry, jobs, nil)
}
func websiteHandlerWithSync(registry *websites.Service, jobs *preparation.Service, syncJobs *analyticssync.Service, publicServices ...*publicsignals.Service) http.HandlerFunc {
	var publicJobs *publicsignals.Service
	if len(publicServices) > 0 {
		publicJobs = publicServices[0]
	}
	queueMarketing := func(w websites.Website) { queueWebsiteAnalytics(syncJobs, w); queueWebsitePublic(publicJobs, w) }
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		parts := strings.Split(strings.TrimPrefix(r.URL.Path, "/api/v1/marketing/websites"), "/")
		if len(parts) == 1 && parts[0] == "" {
			if r.Method == "GET" && r.URL.RawQuery == "" {
				sites, primary := registry.ListWithPrimary(r.Context())
				writeJSON(w, 200, map[string]any{"data": map[string]any{"websites": sites, "primarySource": primary}})
				return
			}
			if r.Method != "POST" {
				websiteError(w, 405)
				return
			}
			body, err := websiteObject(r, "site")
			if err != nil {
				websiteError(w, 400)
				return
			}
			site, ok := objectString(body, "site")
			if !ok {
				websiteError(w, 400)
				return
			}
			value, created, err := registry.Add(r.Context(), site)
			if err != nil {
				websiteServiceError(w, err)
				return
			}
			status := 200
			if created {
				status = 201
				queueWebsitePreparation(jobs, value)
				queueMarketing(value)
			}
			writeJSON(w, status, map[string]any{"data": value})
			return
		}
		if len(parts) < 2 || parts[0] != "" || !websiteIDPattern.MatchString(parts[1]) {
			websiteError(w, 404)
			return
		}
		id := parts[1]
		if len(parts) == 2 && r.Method == "GET" && r.URL.RawQuery == "" {
			record, err := registry.Resolve(id, "")
			if err != nil {
				websiteServiceError(w, err)
				return
			}
			writeJSON(w, 200, map[string]any{"data": record.Website})
			return
		}
		if len(parts) == 4 && parts[2] == "google" && parts[3] == "resources" {
			if r.Method == "GET" && r.URL.RawQuery == "" {
				v, err := registry.GoogleResources(r.Context(), id)
				if err != nil {
					websiteServiceError(w, err)
					return
				}
				writeJSON(w, 200, map[string]any{"data": v})
				return
			}
			if r.Method == "POST" {
				body, err := websiteObject(r, "revision", "resourceId")
				if err != nil {
					websiteError(w, 400)
					return
				}
				choice, ok := objectString(body, "resourceId")
				rev := objectRevision(body)
				if !ok || rev < 1 {
					websiteError(w, 400)
					return
				}
				v, err := registry.SelectGoogleResource(r.Context(), id, choice, rev)
				if err == nil {
					queueMarketing(v)
				}
				websiteResult(w, v, err)
				return
			}
			websiteError(w, 405)
			return
		}
		if len(parts) == 4 && parts[2] == "google" && parts[3] == "status" && r.Method == "GET" {
			query, err := strictQuery(r)
			if err != nil || len(query) != 1 || len(query["sessionId"]) != 1 {
				websiteError(w, 400)
				return
			}
			status, err := registry.GoogleStatus(r.Context(), id, query.Get("sessionId"))
			if err != nil {
				websiteServiceError(w, err)
				return
			}
			writeJSON(w, 200, map[string]any{"data": map[string]string{"status": status}})
			return
		}
		if len(parts) == 2 && r.Method == "PATCH" {
			body, err := websiteObject(r, "site", "revision")
			if err != nil {
				websiteError(w, 400)
				return
			}
			site, ok := objectString(body, "site")
			revision := objectRevision(body)
			if !ok || revision == 0 {
				websiteError(w, 400)
				return
			}
			value, err := registry.Change(r.Context(), id, site, revision)
			if err == nil && value.Revision != revision {
				queueWebsitePreparation(jobs, value)
				queueMarketing(value)
			}
			websiteResult(w, value, err)
			return
		}
		if len(parts) == 3 && parts[2] == "mappings" && r.Method == "PATCH" {
			body, err := websiteObjectOneOf(r, []string{"clarityProjectId", "revision"}, []string{"ga4ReportingSite", "revision"})
			if err != nil {
				websiteError(w, 400)
				return
			}
			revision := objectRevision(body)
			if revision == 0 {
				websiteError(w, 400)
				return
			}
			var value websites.Website
			if body["ga4ReportingSite"] != nil {
				reportingSite, ok := objectString(body, "ga4ReportingSite")
				if !ok {
					websiteError(w, 400)
					return
				}
				value, err = registry.MapGA4ReportingSite(r.Context(), id, reportingSite, revision)
			} else {
				project, ok := objectString(body, "clarityProjectId")
				if !ok {
					websiteError(w, 400)
					return
				}
				value, err = registry.MapClarity(id, project, revision)
			}
			if err == nil {
				queueMarketing(value)
			}
			websiteResult(w, value, err)
			return
		}
		if r.Method != "POST" {
			websiteError(w, 405)
			return
		}
		if _, err := websiteObject(r); err != nil {
			websiteError(w, 400)
			return
		}
		switch strings.Join(parts[2:], "/") {
		case "verify":
			value, err := registry.Verify(r.Context(), id)
			if err == nil {
				queueWebsitePreparation(jobs, value)
				queueMarketing(value)
			}
			websiteResult(w, value, err)
		case "ownership/challenge", "ownership/verify":
			writeJSON(w, http.StatusGone, map[string]any{"error": map[string]string{"code": "dns_workflow_retired", "message": "Connect your provider account to authorize reporting; Studio DNS challenges are retired"}})
		case "google/connect":
			value, err := registry.ConnectGoogle(r.Context(), id)
			if err != nil {
				websiteServiceError(w, err)
				return
			}
			writeJSON(w, 200, map[string]any{"data": value})
		default:
			websiteError(w, 404)
		}
	}
}
func websiteResult(w http.ResponseWriter, value websites.Website, err error) {
	if err != nil {
		websiteServiceError(w, err)
		return
	}
	writeJSON(w, 200, map[string]any{"data": value})
}
func websiteServiceError(w http.ResponseWriter, err error) {
	status := 503
	switch {
	case errors.Is(err, websites.ErrNotFound):
		status = 404
	case errors.Is(err, websites.ErrConflict):
		status = 409
	case errors.Is(err, websites.ErrInvalid):
		status = 400
	}
	websiteError(w, status)
}
func websiteError(w http.ResponseWriter, status int) {
	code, message := "website_unavailable", "Website service is unavailable"
	switch status {
	case 400:
		code, message = "invalid_website_request", "Choose a valid website and request"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before saving"
	case 405:
		code, message = "method_not_allowed", "Method not allowed"
	case 429:
		code, message = "rate_limited", "Too many website requests"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
func objectString(body map[string]json.RawMessage, key string) (string, bool) {
	var result string
	err := json.Unmarshal(body[key], &result)
	return result, err == nil && len(result) > 0 && len(result) <= 512
}
func objectRevision(body map[string]json.RawMessage) int64 {
	var revision int64
	if json.Unmarshal(body["revision"], &revision) != nil || revision < 1 {
		return 0
	}
	return revision
}
func websiteLimiter(next http.HandlerFunc) http.HandlerFunc {
	var mu sync.Mutex
	var until time.Time
	reads, writes := 0, 0
	return func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		now := time.Now()
		if !now.Before(until) {
			until = now.Add(time.Minute)
			reads, writes = 0, 0
		}
		blocked := false
		if r.Method == "GET" {
			reads++
			blocked = reads > 120
		} else {
			writes++
			blocked = writes > 20
		}
		mu.Unlock()
		if blocked {
			w.Header().Set("Retry-After", "60")
			websiteError(w, 429)
			return
		}
		next(w, r)
	}
}
