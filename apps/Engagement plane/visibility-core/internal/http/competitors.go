package http

import (
	"encoding/json"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/competitors"
	"mime"
	"net/http"
)

type competitorSource interface{ Competitors() *competitors.Service }

func registerCompetitors(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(competitorSource)
	if !ok || owner.Competitors() == nil {
		return
	}
	mux.HandleFunc("/api/v1/marketing/competitors", requireInternalKey(key, workLimiter(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var value competitors.Snapshot
		var err error
		if r.Method == "GET" {
			q, e := strictQuery(r)
			if e != nil || len(q) != 2 || len(q["site"]) != 1 || len(q["websiteId"]) != 1 || !websiteIDPattern.MatchString(q.Get("websiteId")) {
				websiteError(w, 400)
				return
			}
			value, err = owner.Competitors().Get(q.Get("websiteId"), q.Get("site"))
		} else if r.Method == "POST" {
			media, _, e := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if e != nil || media != "application/json" {
				websiteError(w, 415)
				return
			}
			fields, e := websiteObjectOneOf(r, []string{"websiteId", "site", "websiteRevision", "target", "operation"}, []string{"websiteId", "site", "websiteRevision", "target", "operation", "ownUrl", "targetUrl"})
			if e != nil {
				websiteError(w, 400)
				return
			}
			id, a := objectString(fields, "websiteId")
			site, b := objectString(fields, "site")
			target, c := objectString(fields, "target")
			operation, d := objectString(fields, "operation")
			var rev int64
			if !a || !b || !c || !d || json.Unmarshal(fields["websiteRevision"], &rev) != nil || rev < 1 {
				websiteError(w, 400)
				return
			}
			paths := []string{}
			if _, ok := fields["ownUrl"]; ok {
				own, oa := objectString(fields, "ownUrl")
				other, ob := objectString(fields, "targetUrl")
				if !oa || !ob {
					websiteError(w, 400)
					return
				}
				paths = []string{own, other}
			} else if _, ok := fields["targetUrl"]; ok {
				websiteError(w, 400)
				return
			}
			value, err = owner.Competitors().Command(id, site, rev, target, operation, paths...)
		} else {
			w.Header().Set("Allow", "GET, POST")
			websiteError(w, 405)
			return
		}
		if err != nil {
			if errors.Is(err, competitors.ErrInvalid) {
				websiteError(w, 400)
			} else if errors.Is(err, competitors.ErrCapacity) {
				websiteError(w, 429)
			} else {
				websiteServiceError(w, err)
			}
			return
		}
		writeJSON(w, 200, map[string]any{"data": value})
	})))
}
