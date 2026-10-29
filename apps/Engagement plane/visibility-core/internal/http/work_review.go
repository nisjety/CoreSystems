package http

import (
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
	"net/http"
)

func reviewWorkRequest(w http.ResponseWriter, r *http.Request, s *workqueue.Service) {
	if r.Method != "POST" {
		workError(w, 405, "method_not_allowed")
		return
	}
	v, err := websiteObject(r, "websiteId", "site", "websiteRevision", "ruleId", "key")
	if err != nil {
		workError(w, 400, "work_invalid_request")
		return
	}
	id, a := objectString(v, "websiteId")
	site, b := objectString(v, "site")
	rule, c := objectString(v, "ruleId")
	key, d := objectString(v, "key")
	var rev int64
	if !a || !b || !c || !d || json.Unmarshal(v["websiteRevision"], &rev) != nil {
		workError(w, 400, "work_invalid_request")
		return
	}
	value, created, err := s.SaveReview(r.Context(), id, site, rev, rule, key)
	status := 200
	if created {
		status = 201
	}
	workResult(w, value, err, status)
}
