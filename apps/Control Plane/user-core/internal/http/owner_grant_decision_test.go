package http

import (
	"net/http"
	"testing"
)

func TestOwnerGrantDecisionIsStrictAndFailsClosedWithoutAuthorityRepository(t *testing.T) {
	s := &Server{}
	for name, body := range map[string]struct {
		body string
		want int
	}{
		"unknown field":                 {`{"space_ref":"space-1","conversation_id":"conversation-1","action_id":"tickets.create","operation":"create","idempotency_key":"grant-1","unexpected":true}`, http.StatusBadRequest},
		"valid shape but no repository": {`{"space_ref":"space-1","conversation_id":"conversation-1","action_id":"tickets.create","operation":"create","idempotency_key":"grant-1"}`, http.StatusServiceUnavailable},
	} {
		t.Run(name, func(t *testing.T) {
			c, w := newGinJSONCtx(body.body)
			s.issueOwnerGrantDecision(c)
			if w.Code != body.want {
				t.Fatalf("status/body = %d/%s, want %d", w.Code, w.Body.String(), body.want)
			}
		})
	}
}
