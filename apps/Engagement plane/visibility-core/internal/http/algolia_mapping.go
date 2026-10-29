package http

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/triodelab/engagement-plane/visibility-core/internal/algoliamapping"
	"github.com/triodelab/engagement-plane/visibility-core/internal/sitesources"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
	"net/http"
	"time"
)

type algoliaMappingSource interface {
	AlgoliaMapping() *algoliamapping.Service
}

func registerAlgoliaMapping(mux *http.ServeMux, key string, source PulseGetter) {
	owner, ok := source.(algoliaMappingSource)
	if !ok || owner.AlgoliaMapping() == nil {
		return
	}
	var sources *sitesources.Service
	if provider, ok := source.(siteSourcesSource); ok {
		sources = provider.SiteSources()
	}
	mux.HandleFunc("/api/v1/marketing/site-sources/algolia-mapping", requireInternalKey(key, workLimiter(algoliaMappingHandler(owner.AlgoliaMapping(), sources))))
}
func algoliaMappingHandler(service *algoliamapping.Service, sources ...*sitesources.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var data algoliamapping.Mapping
		var err error
		switch r.Method {
		case "GET":
			q, e := strictQuery(r)
			if e != nil || len(q) != 2 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || r.ContentLength != 0 || len(r.TransferEncoding) > 0 {
				algoliaMappingError(w, 400)
				return
			}
			data, err = service.Get(r.Context(), q.Get("websiteId"), q.Get("site"))
		case "PUT", "DELETE":
			if e := http.NewResponseController(w).SetWriteDeadline(time.Now().Add(24 * time.Second)); e != nil && !errors.Is(e, http.ErrNotSupported) {
				algoliaMappingError(w, 503)
				return
			}
			fields := []string{"websiteId", "site", "websiteRevision"}
			if r.Method == "PUT" {
				fields = append(fields, "connectionId", "applicationId", "indexName")
			}
			body, e := websiteObject(r, fields...)
			if e != nil {
				algoliaMappingError(w, 400)
				return
			}
			id, a := objectString(body, "websiteId")
			site, b := objectString(body, "site")
			var revision int64
			if !a || !b || json.Unmarshal(body["websiteRevision"], &revision) != nil || revision < 1 {
				algoliaMappingError(w, 400)
				return
			}
			if r.Method == "DELETE" {
				data, err = service.Delete(id, site, revision)
			} else {
				connection, c := objectString(body, "connectionId")
				app, d := objectString(body, "applicationId")
				index, f := objectString(body, "indexName")
				if !c || !d || !f {
					algoliaMappingError(w, 400)
					return
				}
				ctx, cancel := context.WithTimeout(r.Context(), 22*time.Second)
				data, err = service.Put(ctx, id, site, revision, connection, app, index)
				cancel()
				if err == nil && len(sources) > 0 && sources[0] != nil {
					_, err = sources[0].QueueCollection(id, site, revision, "algolia")
				}
			}
		default:
			w.Header().Set("Allow", "GET, PUT, DELETE")
			algoliaMappingError(w, 405)
			return
		}
		if err != nil {
			status := 503
			switch {
			case errors.Is(err, algoliamapping.ErrInvalid), errors.Is(err, websites.ErrInvalid):
				status = 400
			case errors.Is(err, algoliamapping.ErrForbidden):
				status = 403
			case errors.Is(err, websites.ErrNotFound):
				status = 404
			case errors.Is(err, websites.ErrConflict):
				status = 409
			}
			algoliaMappingError(w, status)
			return
		}
		writeJSON(w, 200, map[string]any{"data": data})
	}
}
func algoliaMappingError(w http.ResponseWriter, status int) {
	code, message := "algolia_mapping_unavailable", "Algolia mapping could not be checked or saved; retry later"
	switch status {
	case 400:
		code, message = "invalid_algolia_mapping", "Choose a current owned website and allowed Algolia resource"
	case 403:
		code, message = "provider_authorization_required", "Connect an authorized Algolia account and select an accessible resource"
	case 404:
		code, message = "website_not_found", "Website was not found"
	case 409:
		code, message = "website_conflict", "Website or mapping changed; reload before saving"
	case 405:
		code, message = "method_not_allowed", "Use GET, PUT or DELETE"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
