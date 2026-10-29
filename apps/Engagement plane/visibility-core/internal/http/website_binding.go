package http

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
)

// bindWebsite resolves the immutable website ID inside the fixed organization.
// Legacy raw-site reads remain supported only for a registered website.
func bindWebsite(source PulseGetter, next http.HandlerFunc) http.HandlerFunc {
	provider, enabled := source.(websiteSource)
	if !enabled || provider.WebsiteRegistry() == nil {
		return next
	}
	return func(w http.ResponseWriter, r *http.Request) {
		query, err := strictQuery(r)
		if err != nil {
			websiteError(w, 400)
			return
		}
		id, site := query.Get("websiteId"), query.Get("site")
		copyRequest := r.Clone(r.Context())
		if r.Method == "POST" && r.URL.Path == "/api/v1/marketing/pagespeed" {
			body, err := io.ReadAll(io.LimitReader(r.Body, 4097))
			if err != nil || len(body) > 4096 {
				websiteError(w, 400)
				return
			}
			fields := []string{"site", "strategy"}
			var probe map[string]json.RawMessage
			if json.Unmarshal(body, &probe) == nil && probe["websiteId"] != nil {
				fields = append(fields, "websiteId")
			}
			object, err := strictObject(body, fields...)
			if err != nil {
				websiteError(w, 400)
				return
			}
			var ok bool
			site, ok = objectString(object, "site")
			if !ok {
				websiteError(w, 400)
				return
			}
			if object["websiteId"] != nil {
				id, ok = objectString(object, "websiteId")
				if !ok {
					websiteError(w, 400)
					return
				}
			}
			delete(object, "websiteId")
			body, _ = json.Marshal(object)
			copyRequest.Body = io.NopCloser(bytes.NewReader(body))
			copyRequest.ContentLength = int64(len(body))
		} else {
			if len(query["websiteId"]) > 1 {
				websiteError(w, 400)
				return
			}
			query.Del("websiteId")
			copyRequest.URL.RawQuery = query.Encode()
		}
		if id != "" && !websiteIDPattern.MatchString(id) {
			websiteError(w, 400)
			return
		}
		record, err := provider.WebsiteRegistry().Resolve(id, site)
		if err != nil {
			websiteServiceError(w, err)
			return
		}
		if site == "" || record.Website.Site != site {
			websiteError(w, 409)
			return
		}
		next(w, copyRequest)
	}
}
