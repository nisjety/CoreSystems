package http

import (
	"net/http"
	"testing"
)

// The view route must reject malformed input before any Control lookup and
// fail closed when its independent Session/Core evidence dependencies are not
// configured. A browser cannot turn this into a direct catalog endpoint.
func TestIssueModelActionViewRejectsMalformedAndUnwiredRequests(t *testing.T) {
	s := &Server{}
	for name, test := range map[string]struct {
		body string
		want int
	}{
		"unknown field": {`{"run_id":"run-1","org_id":"org-1","action_id":"tickets.create"}`, http.StatusBadRequest},
		"missing org":   {`{"run_id":"run-1"}`, http.StatusBadRequest},
		"unwired":       {`{"run_id":"run-1","org_id":"org-1"}`, http.StatusServiceUnavailable},
	} {
		t.Run(name, func(t *testing.T) {
			c, w := newGinJSONCtx(test.body)
			s.issueModelActionView(c)
			if w.Code != test.want {
				t.Fatalf("status = %d, want %d: %s", w.Code, test.want, w.Body.String())
			}
		})
	}
}
