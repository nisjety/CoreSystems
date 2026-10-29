package http

import (
	"encoding/json"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
)

func siteSourceCollection(service *sitesources.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var data sitesources.CollectionState
		var err error
		status := 200
		switch r.Method {
		case "GET":
			q, e := strictQuery(r)
			if e != nil || len(q) != 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["provider"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
				siteSourcesError(w, 400)
				return
			}
			data, err = service.Collection(r.Context(), q.Get("websiteId"), q.Get("site"), q.Get("provider"))
		case "POST":
			body, e := websiteObject(r, "websiteId", "site", "websiteRevision", "provider")
			if e != nil {
				siteSourcesError(w, 400)
				return
			}
			id, a := objectString(body, "websiteId")
			site, b := objectString(body, "site")
			provider, c := objectString(body, "provider")
			var revision int64
			if !a || !b || !c || json.Unmarshal(body["websiteRevision"], &revision) != nil || revision < 1 {
				siteSourcesError(w, 400)
				return
			}
			data, err = service.QueueCollection(id, site, revision, provider)
			status = http.StatusAccepted
		default:
			w.Header().Set("Allow", "GET, POST")
			siteSourcesError(w, 405)
			return
		}
		if err != nil {
			siteSourcesServiceError(w, err)
			return
		}
		writeJSON(w, status, map[string]any{"data": data})
	}
}
