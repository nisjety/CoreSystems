package http

import (
	"errors"
	"net/http"
	"net/url"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

type weeklyResponse struct {
	siteaudit.WeeklyReport
	TasksAsOf string             `json:"tasksAsOf"`
	Work      *workqueue.Summary `json:"work"`
}

func registerInsights(mux *http.ServeMux, key string, source PulseGetter) {
	provider, ok := source.(auditSource)
	if !ok || provider.Audit() == nil {
		return
	}
	var work *workqueue.Service
	if owner, ok := source.(workSource); ok {
		work = owner.WorkQueue()
	}
	mux.HandleFunc("/api/v1/marketing/audit-insights", requireInternalKey(key, workLimiter(insightHandler(provider.Audit(), work, false))))
	mux.HandleFunc("/api/v1/marketing/reports/weekly", requireInternalKey(key, workLimiter(insightHandler(provider.Audit(), work, true))))
}
func insightQuery(r *http.Request, weekly bool) (url.Values, error) {
	q, err := strictQuery(r)
	if err != nil || len(q) < 2 || len(q) > 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 {
		return nil, siteaudit.ErrInvalid
	}
	for key, values := range q {
		if len(values) != 1 || values[0] == "" || key != "websiteId" && key != "site" && (!weekly || key != "weekStart") {
			return nil, siteaudit.ErrInvalid
		}
	}
	return q, nil
}
func insightHandler(audits *siteaudit.Service, work *workqueue.Service, weekly bool) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != http.MethodGet {
			w.Header().Set("Allow", "GET")
			insightError(w, 405)
			return
		}
		query, err := insightQuery(r, weekly)
		if err != nil {
			insightError(w, 400)
			return
		}
		id, site := query.Get("websiteId"), query.Get("site")
		var result any
		if weekly {
			var report siteaudit.WeeklyReport
			report, err = audits.Weekly(id, site, query.Get("weekStart"))
			value := weeklyResponse{WeeklyReport: report}
			if err == nil && work != nil {
				var queue workqueue.Snapshot
				queue, err = work.List(id, site)
				if err == nil {
					value.Work = &queue.Summary
					value.TasksAsOf = report.AsOf
				}
			}
			result = value
		} else {
			result, err = audits.Insights(id, site)
		}
		if err != nil {
			insightError(w, insightErrorStatus(err))
			return
		}
		writeJSON(w, 200, map[string]any{"data": result})
	}
}
func insightErrorStatus(err error) int {
	switch {
	case errors.Is(err, siteaudit.ErrInvalid) || errors.Is(err, websites.ErrInvalid) || errors.Is(err, workqueue.ErrInvalid):
		return 400
	case errors.Is(err, siteaudit.ErrNotFound) || errors.Is(err, websites.ErrNotFound) || errors.Is(err, workqueue.ErrNotFound):
		return 404
	case errors.Is(err, siteaudit.ErrConflict) || errors.Is(err, websites.ErrConflict) || errors.Is(err, workqueue.ErrConflict):
		return 409
	default:
		return 503
	}
}
func insightError(w http.ResponseWriter, status int) {
	code, message := "audit_history_unavailable", "Audit history is unavailable; refresh to confirm the latest observations"
	switch status {
	case 400:
		code, message = "invalid_audit_history_request", "Choose a registered website and a current or past UTC Monday"
	case 404:
		code, message = "audit_history_not_found", "Registered website was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload its history"
	case 405:
		code, message = "method_not_allowed", "GET is required"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
