package http

import (
	"encoding/json"
	"net/http"

	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
)

func technicalWorkRequest(w http.ResponseWriter, r *http.Request, service *workqueue.Service) {
	if r.Method != http.MethodPost {
		w.Header().Set("Allow", "POST")
		workError(w, 405, "method_not_allowed")
		return
	}
	body, err := websiteObject(r, "websiteId", "site", "websiteRevision", "strategy", "checkId", "fetchTime")
	if err != nil {
		workError(w, 400, "work_invalid_request")
		return
	}
	id, site, revision, ok := workContext(body)
	strategy, a := objectString(body, "strategy")
	check, b := objectString(body, "checkId")
	stamp, c := objectString(body, "fetchTime")
	if !ok || !a || !b || !c {
		workError(w, 400, "work_invalid_request")
		return
	}
	value, created, err := service.SaveTechnical(r.Context(), id, site, revision, strategy, check, stamp)
	status := 200
	if created {
		status = 201
	}
	workResult(w, value, err, status)
}
func verifyWorkRequest(w http.ResponseWriter, r *http.Request, service *workqueue.Service, id string) {
	if r.Method != http.MethodPost {
		w.Header().Set("Allow", "POST")
		workError(w, 405, "method_not_allowed")
		return
	}
	body, err := websiteObject(r, "websiteId", "site", "websiteRevision", "revision")
	if err != nil {
		workError(w, 400, "work_invalid_request")
		return
	}
	websiteID, site, websiteRevision, ok := workContext(body)
	revision := objectRevision(body)
	if !ok || revision < 1 {
		workError(w, 400, "work_invalid_request")
		return
	}
	value, err := service.Verify(r.Context(), websiteID, site, websiteRevision, id, revision)
	workResult(w, value, err, 200)
}
func workContext(body map[string]json.RawMessage) (string, string, int64, bool) {
	id, a := objectString(body, "websiteId")
	site, b := objectString(body, "site")
	var revision int64
	err := json.Unmarshal(body["websiteRevision"], &revision)
	return id, site, revision, a && b && err == nil && revision > 0
}
