package http

import (
	"context"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
	"github.com/triodelab/engagement-plane/visibility-core/internal/visibilityanalysis"
	"net/http"
	"time"
)

type analysisSource interface {
	VisibilityAnalysis(context.Context, string, string, string) (visibilityanalysis.Snapshot, error)
}

func registerVisibilityAnalysis(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(analysisSource)
	if !ok {
		return
	}
	mux.HandleFunc("/api/v1/marketing/analysis", requireInternalKey(key, workLimiter(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != "GET" {
			websiteError(w, 405)
			return
		}
		q, err := strictQuery(r)
		if err != nil || len(q) != 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["quarter"]) != 1 || !websiteIDPattern.MatchString(q.Get("websiteId")) {
			websiteError(w, 400)
			return
		}
		if _, err = pulse.QuarterPeriod(q.Get("quarter"), time.Now()); err != nil {
			websiteError(w, 400)
			return
		}
		value, err := owner.VisibilityAnalysis(r.Context(), q.Get("websiteId"), q.Get("site"), q.Get("quarter"))
		if err != nil {
			websiteServiceError(w, err)
			return
		}
		writeJSON(w, 200, map[string]any{"data": value})
	})))
}
