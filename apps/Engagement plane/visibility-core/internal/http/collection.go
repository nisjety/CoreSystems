package http

import (
	"context"
	"net/http"
	"time"

	"github.com/triodelab/engagement-plane/visibility-core/internal/collection"
	"github.com/triodelab/engagement-plane/visibility-core/internal/pulse"
)

type collectionSource interface {
	CollectionInventory(context.Context, string, string, string) (collection.Inventory, error)
}

func registerCollection(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(collectionSource)
	if !ok {
		return
	}
	mux.HandleFunc("/api/v1/marketing/collection", requireInternalKey(key, workLimiter(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		if r.Method != "GET" {
			websiteError(w, 405)
			return
		}
		q, err := strictQuery(r)
		if err != nil || len(q) != 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["quarter"]) != 1 || !websiteIDPattern.MatchString(q.Get("websiteId")) || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
			websiteError(w, 400)
			return
		}
		if _, err = pulse.QuarterPeriod(q.Get("quarter"), time.Now()); err != nil {
			websiteError(w, 400)
			return
		}
		value, err := owner.CollectionInventory(r.Context(), q.Get("websiteId"), q.Get("site"), q.Get("quarter"))
		if err != nil {
			websiteServiceError(w, err)
			return
		}
		writeJSON(w, 200, map[string]any{"data": value})
	})))
}
