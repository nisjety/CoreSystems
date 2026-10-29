package http

import (
	"context"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/outcomes"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http"
)

type outcomesSource interface {
	BusinessOutcomes(context.Context, string, string, string, string) (outcomes.Projection, error)
}

func registerOutcomes(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(outcomesSource)
	if !ok {
		return
	}
	mux.HandleFunc("/api/v1/marketing/outcomes", requireInternalKey(key, workLimiter(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != "GET" {
			w.Header().Set("Allow", "GET")
			writeJSON(w, 405, map[string]any{"error": map[string]string{"code": "method_not_allowed", "message": "GET is required"}})
			return
		}
		q, err := strictQuery(r)
		if err != nil || len(q) != 4 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["quarter"]) != 1 || len(q["environment"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
			webVitalsError(w, 400)
			return
		}
		data, err := owner.BusinessOutcomes(r.Context(), q.Get("websiteId"), q.Get("site"), q.Get("quarter"), q.Get("environment"))
		if err != nil {
			status := 503
			if errors.Is(err, outcomes.ErrInvalid) {
				status = 400
			}
			if errors.Is(err, websites.ErrNotFound) {
				status = 404
			}
			if errors.Is(err, websites.ErrConflict) {
				status = 409
			}
			writeJSON(w, status, map[string]any{"error": map[string]string{"code": "outcomes_unavailable", "message": "Choose the current registered website and period"}})
			return
		}
		writeJSON(w, 200, map[string]any{"data": data})
	})))
}
