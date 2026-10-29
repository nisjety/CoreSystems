package http

import (
	"encoding/json"
	"errors"
	"mime"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/siteaudit"
	"github.com/triodelab/engagement-plane/visibility-core/internal/websites"
)

type auditSource interface{ Audit() *siteaudit.Service }

func registerAudit(mux *http.ServeMux, key string, source PulseGetter) {
	provider, ok := source.(auditSource)
	if !ok || provider.Audit() == nil {
		return
	}
	mux.HandleFunc("/api/v1/marketing/audits", requireInternalKey(key, workLimiter(auditHandler(provider.Audit()))))
}
func auditHandler(service *siteaudit.Service) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		var id, site, auditID string
		var revision int64
		switch r.Method {
		case http.MethodGet:
			q, err := strictQuery(r)
			if err != nil || len(q) < 2 || len(q) > 3 || len(q["websiteId"]) != 1 || len(q["site"]) != 1 || len(q["auditId"]) > 1 {
				auditError(w, 400)
				return
			}
			for name := range q {
				if name != "websiteId" && name != "site" && name != "auditId" {
					auditError(w, 400)
					return
				}
			}
			id, site, auditID = q.Get("websiteId"), q.Get("site"), q.Get("auditId")
			if len(q["auditId"]) == 1 && auditID == "" {
				auditError(w, 400)
				return
			}
		case http.MethodPost:
			media, _, err := mime.ParseMediaType(r.Header.Get("Content-Type"))
			if err != nil || media != "application/json" {
				auditError(w, 415)
				return
			}
			fields, err := websiteObject(r, "websiteId", "site", "websiteRevision")
			if err != nil {
				auditError(w, 400)
				return
			}
			var a, b bool
			id, a = objectString(fields, "websiteId")
			site, b = objectString(fields, "site")
			if !a || !b || json.Unmarshal(fields["websiteRevision"], &revision) != nil || revision < 1 {
				auditError(w, 400)
				return
			}
		default:
			w.Header().Set("Allow", "GET, POST")
			auditError(w, 405)
			return
		}
		var v siteaudit.Snapshot
		var err error
		if r.Method == http.MethodPost {
			v, err = service.Start(id, site, revision)
		} else if auditID != "" {
			v, err = service.GetRun(id, site, auditID)
		} else {
			v, err = service.Get(id, site)
		}
		if err != nil {
			status := 503
			switch {
			case errors.Is(err, siteaudit.ErrInvalid) || errors.Is(err, websites.ErrInvalid):
				status = 400
			case errors.Is(err, siteaudit.ErrNotFound) || errors.Is(err, websites.ErrNotFound):
				status = 404
			case errors.Is(err, siteaudit.ErrConflict) || errors.Is(err, websites.ErrConflict):
				status = 409
			case errors.Is(err, siteaudit.ErrCapacity):
				status = 429
				w.Header().Set("Retry-After", "60")
			}
			auditError(w, status)
			return
		}
		writeJSON(w, 200, map[string]any{"data": v})
	}
}
func auditError(w http.ResponseWriter, status int) {
	code, message := "audit_unavailable", "Website audit is unavailable"
	switch status {
	case 400:
		code, message = "invalid_audit_request", "Choose a registered website and audit"
	case 404:
		code, message = "audit_not_found", "Website audit was not found"
	case 409:
		code, message = "website_conflict", "Website changed; reload before auditing"
	case 415:
		code, message = "invalid_content_type", "JSON is required"
	case 405:
		code, message = "method_not_allowed", "GET or POST is required"
	case 429:
		code, message = "rate_limited", "Wait before starting another website audit"
	}
	writeJSON(w, status, map[string]any{"error": map[string]string{"code": code, "message": message}})
}
