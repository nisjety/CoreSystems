package http

import (
	"encoding/json"
	"github.com/triodelab/engagement-plane/visibility-core/internal/workqueue"
	"net/http"
)

func planningWorkRequest(w http.ResponseWriter, r *http.Request, service *workqueue.Service, id string) {
	if r.Method != "PATCH" {
		workError(w, 405, "method_not_allowed")
		return
	}
	body, err := websiteObject(r, "websiteId", "site", "websiteRevision", "revision", "assigneeId", "dueDate", "priority")
	if err != nil {
		workError(w, 400, "work_invalid_request")
		return
	}
	website, a := objectString(body, "websiteId")
	site, b := objectString(body, "site")
	assignee, c := objectString(body, "assigneeId")
	due, d := objectString(body, "dueDate")
	priority, e := objectString(body, "priority")
	var rev int64
	if !a || !b || !c || !d || !e || json.Unmarshal(body["websiteRevision"], &rev) != nil {
		workError(w, 400, "work_invalid_request")
		return
	}
	value, err := service.Plan(website, site, rev, id, objectRevision(body), workqueue.Planning{AssigneeID: assignee, DueDate: due, Priority: priority})
	workResult(w, value, err, 200)
}
