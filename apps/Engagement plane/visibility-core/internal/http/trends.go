package http

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/trends"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"mime"
	"net/http"
	"time"
)

type trendsSource interface {
	GetTrends(context.Context, string, string, int64, trends.Query) (trends.Report, error)
}
type automaticTrendsSource interface{ AutomaticTrends() *trends.Automatic }

func registerTrends(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(trendsSource)
	if !ok {
		return
	}
	mux.HandleFunc("/api/v1/marketing/trends", requireInternalKey(key, workLimiter(trendsHandler(owner))))
	if automatic, ok := source.(automaticTrendsSource); ok && automatic.AutomaticTrends() != nil {
		mux.HandleFunc("/api/v1/marketing/trends/automatic", requireInternalKey(key, workLimiter(automaticTrendsHandler(automatic.AutomaticTrends()))))
	}
}
func automaticTrendsHandler(service *trends.Automatic) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != "GET" {
			w.Header().Set("Allow", "GET")
			websiteError(w, 405)
			return
		}
		q, err := strictQuery(r)
		if err != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
			websiteError(w, 400)
			return
		}
		snapshot, err := service.Get(r.Context(), q.Get("websiteId"), q.Get("site"))
		if err != nil {
			websiteServiceError(w, err)
			return
		}
		writeJSON(w, 200, map[string]any{"data": snapshot})
	}
}
func trendsHandler(owner trendsSource) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != "POST" {
			w.Header().Set("Allow", "POST")
			websiteError(w, 405)
			return
		}
		if err := http.NewResponseController(w).SetWriteDeadline(time.Now().Add(36 * time.Second)); err != nil && !errors.Is(err, http.ErrNotSupported) {
			siteSourcesError(w, 503)
			return
		}
		media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
		if err != nil || media != "application/json" {
			siteSourcesError(w, 415)
			return
		}
		body, err := websiteObject(r, "websiteId", "site", "websiteRevision", "terms", "geo", "timeframe")
		if err != nil {
			siteSourcesError(w, 400)
			return
		}
		id, a := objectString(body, "websiteId")
		site, b := objectString(body, "site")
		var geo string
		c := json.Unmarshal(body["geo"], &geo) == nil
		frame, d := objectString(body, "timeframe")
		var terms []string
		var revision int64
		if !a || !b || !c || !d || json.Unmarshal(body["websiteRevision"], &revision) != nil || json.Unmarshal(body["terms"], &terms) != nil || !websiteIDPattern.MatchString(id) || revision < 1 {
			siteSourcesError(w, 400)
			return
		}
		q := trends.Query{Terms: terms, Geo: geo, Timeframe: frame}
		if !trends.Valid(q) {
			siteSourcesError(w, 400)
			return
		}
		report, err := owner.GetTrends(r.Context(), id, site, revision, q)
		if err != nil {
			if errors.Is(err, websites.ErrConflict) || errors.Is(err, websites.ErrNotFound) {
				websiteServiceError(w, err)
			} else {
				siteSourcesError(w, 503)
			}
			return
		}
		writeJSON(w, 200, map[string]any{"data": map[string]any{"websiteId": id, "site": site, "websiteRevision": revision, "report": report}})
	}
}
