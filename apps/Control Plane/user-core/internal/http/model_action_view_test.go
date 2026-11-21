package http

import (
	"context"
	"errors"
	"net/http"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// countingRunAuthority is a run-authority lookup that always fails and counts its calls, so a test can see
// whether a request reached Control at all.
type countingRunAuthority struct {
	calls int
}

func (c *countingRunAuthority) ResolveRunActionAuthority(context.Context, string, string) (spaces.RunActionAuthority, error) {
	c.calls++
	return spaces.RunActionAuthority{}, errors.New("authority unavailable in this test")
}

// The v2 request names its actions in action_ids. A malformed list (empty, a blank or overlong id, more than the
// cap, or not a list) is refused before any Control lookup.
func TestIssueModelActionViewRefusesMalformedActionIdsBeforeAnyLookup(t *testing.T) {
	overlong := `"` + strings.Repeat("a", 201) + `"`
	tooMany := strings.TrimSuffix(strings.Repeat(`"tickets.create",`, 17), ",")
	lookup := &countingRunAuthority{}
	s := &Server{spaceRepo: &spaces.Repository{}, runActionAuthority: lookup}
	for name, list := range map[string]string{
		"empty list":   `[]`,
		"blank id":     `["  "]`,
		"overlong id":  `[` + overlong + `]`,
		"too many ids": `[` + tooMany + `]`,
		"not a list":   `"tickets.create"`,
	} {
		t.Run(name, func(t *testing.T) {
			c, w := newGinJSONCtx(`{"run_id":"run-1","org_id":"org-1","action_ids":` + list + `}`)
			s.issueModelActionView(c)
			if w.Code != http.StatusBadRequest {
				t.Fatalf("status = %d, want 400: %s", w.Code, w.Body.String())
			}
		})
	}
	if lookup.calls != 0 {
		t.Fatalf("a malformed action_ids list reached the Control lookup %d times", lookup.calls)
	}
}

// Well-formed v1 and v2 requests pass the decoder and the bounds and reach the Control lookup. The lookup here
// fails, so each answers 503 and none reaches a view. The full issuance of a v2 view is covered where the
// evidence can be built: the spaces package tests IssueModelActionViewV2 in both modes.
func TestIssueModelActionViewReachesTheLookupForV1AndV2Requests(t *testing.T) {
	lookup := &countingRunAuthority{}
	s := &Server{spaceRepo: &spaces.Repository{}, runActionAuthority: lookup}
	for name, body := range map[string]string{
		"v1":           `{"run_id":"run-1","org_id":"org-1"}`,
		"v2":           `{"run_id":"run-1","org_id":"org-1","action_ids":["tickets.create"]}`,
		"v2 null list": `{"run_id":"run-1","org_id":"org-1","action_ids":null}`,
	} {
		t.Run(name, func(t *testing.T) {
			c, w := newGinJSONCtx(body)
			s.issueModelActionView(c)
			if w.Code != http.StatusServiceUnavailable {
				t.Fatalf("status = %d, want 503 from the failing lookup: %s", w.Code, w.Body.String())
			}
		})
	}
	if lookup.calls != 3 {
		t.Fatalf("every well-formed request must reach the Control lookup: %d calls", lookup.calls)
	}
}

// RUN_ACTION_TARGETS is read once at construction. Only the exact literal "table" widens the admitted owner
// actions; unset, empty and any other value keep tickets_only.
func TestRunActionTargetsIsReadOnceAtConstruction(t *testing.T) {
	cases := map[string]spaces.RunActionTargetsMode{
		"table":        spaces.RunActionTargetsTable,
		"tickets_only": spaces.RunActionTargetsTicketsOnly,
		"TABLE":        spaces.RunActionTargetsTicketsOnly,
		" table":       spaces.RunActionTargetsTicketsOnly,
		"true":         spaces.RunActionTargetsTicketsOnly,
		"":             spaces.RunActionTargetsTicketsOnly,
	}
	for value, want := range cases {
		t.Setenv("RUN_ACTION_TARGETS", value)
		if got := NewServer(nil, nil, nil, nil, "0").runActionTargets; got != want {
			t.Fatalf("RUN_ACTION_TARGETS=%q: mode = %q, want %q", value, got, want)
		}
	}
}

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
