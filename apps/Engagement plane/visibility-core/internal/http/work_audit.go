package http

import (
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

// The client selects references only. Marketing Core resolves all rule evidence
// from its authorized persisted audit, not from submitted scores or page data.
func auditWorkRequest(w http.ResponseWriter, r *http.Request, service *workqueue.Service) {
	if r.Method != http.MethodPost {
		w.Header().Set("Allow", "POST")
		workError(w, 405, "method_not_allowed")
		return
	}
	body, err := websiteObject(r, "websiteId", "site", "websiteRevision", "auditId", "findingId")
	if err != nil {
		workError(w, 400, "work_invalid_request")
		return
	}
	id, site, revision, ok := workContext(body)
	auditID, a := objectString(body, "auditId")
	findingID, b := objectString(body, "findingId")
	if !ok || !a || !b {
		workError(w, 400, "work_invalid_request")
		return
	}
	value, created, err := service.SaveAudit(r.Context(), id, site, revision, auditID, findingID)
	status := 200
	if created {
		status = 201
	}
	workResult(w, value, err, status)
}
