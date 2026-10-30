package http

import (
	"encoding/json"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/searchresearch"
	"net/http"
)

type researchSource interface {
	SearchResearch() *searchresearch.Service
}

func registerSearchResearch(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(researchSource)
	if !ok || owner.SearchResearch() == nil {
		return
	}
	handler := requireInternalKey(key, workLimiter(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var value searchresearch.Snapshot
		var err error
		if r.Method == "GET" && r.URL.Path == "/api/v1/marketing/search/research" {
			q, e := strictQuery(r)
			if e != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 {
				websiteError(w, 400)
				return
			}
			value, err = owner.SearchResearch().Get(r.Context(), q.Get("websiteId"), q.Get("site"))
		} else if r.Method == "POST" {
			fields := []string{"websiteId", "site", "websiteRevision"}
			isRule := r.URL.Path == "/api/v1/marketing/search/rules"
			if isRule {
				fields = append(fields, "operation", "ruleId", "name", "entity", "contains", "minImpressions", "maxPosition")
			}
			body, e := websiteObject(r, fields...)
			if e != nil {
				websiteError(w, 400)
				return
			}
			id, a := objectString(body, "websiteId")
			site, b := objectString(body, "site")
			var rev int64
			if !a || !b || json.Unmarshal(body["websiteRevision"], &rev) != nil || rev < 1 {
				websiteError(w, 400)
				return
			}
			operation := "refresh"
			var rule *searchresearch.Rule
			if isRule {
				var v searchresearch.Rule
				operation, _ = objectString(body, "operation")
				v.ID, _ = objectString(body, "ruleId")
				v.Name, _ = objectString(body, "name")
				v.Entity, _ = objectString(body, "entity")
				v.Contains, _ = objectString(body, "contains")
				if json.Unmarshal(body["minImpressions"], &v.MinImpressions) != nil || json.Unmarshal(body["maxPosition"], &v.MaxPosition) != nil {
					websiteError(w, 400)
					return
				}
				rule = &v
			}
			value, err = owner.SearchResearch().Command(r.Context(), id, site, rev, operation, rule)
		} else {
			websiteError(w, 405)
			return
		}
		if err != nil {
			if errors.Is(err, searchresearch.ErrInvalid) {
				websiteError(w, 400)
			} else if errors.Is(err, searchresearch.ErrCapacity) {
				websiteError(w, 429)
			} else {
				websiteServiceError(w, err)
			}
			return
		}
		writeJSON(w, 200, map[string]any{"data": value})
	}))
	mux.HandleFunc("/api/v1/marketing/search/research", handler)
	mux.HandleFunc("/api/v1/marketing/search/rules", handler)
}
