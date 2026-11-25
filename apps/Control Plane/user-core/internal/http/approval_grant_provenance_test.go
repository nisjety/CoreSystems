package http

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// WS10-S2: the standing-rule provenance refusals on the wire, and the rule that a
// standing rule names its approval before any authority lookup. Each case stops
// before the database or uses a fake Session Core projection.

// fakeApprovalAuthority stands in for Session Core's approval projection.
type fakeApprovalAuthority struct {
	authority spaces.ApprovalDecideAuthority
	err       error
}

func (f fakeApprovalAuthority) ResolveApprovalAuthority(context.Context, string, string) (spaces.ApprovalDecideAuthority, error) {
	return f.authority, f.err
}

func TestWriteApprovalErrorMapsTheStandingRuleProvenanceRefusals(t *testing.T) {
	gin.SetMode(gin.TestMode)
	for _, tc := range []struct {
		name   string
		err    error
		status int
		code   string
	}{
		{"not resolved", spaces.ErrApprovalAuthorityNotResolved, http.StatusNotFound, "approval_not_found"},
		{"not visible", spaces.ErrApprovalNotFound, http.StatusNotFound, "approval_not_found"},
		{"not granted", spaces.ErrApprovalNotGranted, http.StatusConflict, "approval_not_granted"},
		{"wrapped not granted", fmt.Errorf("verify: %w", spaces.ErrApprovalNotGranted), http.StatusConflict, "approval_not_granted"},
		{"authority client failure", errors.New("session core unreachable"), http.StatusServiceUnavailable, "approval_authority_unavailable"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := httptest.NewRecorder()
			c, _ := gin.CreateTestContext(rec)
			c.Request = httptest.NewRequest(http.MethodPost, "/", nil)
			writeApprovalError(c, tc.err)
			wantApprovalError(t, rec, tc.status, tc.code)
			if strings.Contains(rec.Body.String(), "unreachable") {
				t.Fatalf("an internal error message leaked: %s", rec.Body.String())
			}
		})
	}
}

// provenanceEvidence is the caller's resolved authority: a manager of space-room in
// org-1, user-1.
func provenanceEvidence() spaces.PersonalThreadDecisionEvidence {
	return spaces.PersonalThreadDecisionEvidence{Membership: spaces.CurrentMembership{
		SpaceRef: "space-room", OrgID: "org-1", SubjectID: "user-1", Kind: spaces.KindRoom, Role: "manager",
	}}
}

func grantedProvenanceAuthority() spaces.ApprovalDecideAuthority {
	return spaces.ApprovalDecideAuthority{
		ApprovalID: "approval-1", OrgID: "org-1", SpaceRef: "space-room", RunID: "run-1",
		RequesterSubjectID: "user-1", AddresseeKind: "requester", State: "granted",
	}
}

// runProvenance runs the provenance check for approval-1 and reports what it wrote.
func runProvenance(t *testing.T, server *Server) (spaces.ApprovalDecideAuthority, bool, *httptest.ResponseRecorder) {
	t.Helper()
	rec := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(rec)
	c.Request = httptest.NewRequest(http.MethodPost, "/", nil)
	authority, ok := server.verifyStandingRuleProvenance(c, provenanceEvidence(), "approval-1")
	return authority, ok, rec
}

func TestVerifyStandingRuleProvenanceAnswersTheTypedRefusals(t *testing.T) {
	gin.SetMode(gin.TestMode)
	requested := grantedProvenanceAuthority()
	requested.State = "requested"
	elsewhere := grantedProvenanceAuthority()
	elsewhere.SpaceRef = "space-other"
	for _, tc := range []struct {
		name     string
		resolver ApprovalAuthorityResolver
		status   int
		code     string
	}{
		{"no resolver", nil, http.StatusServiceUnavailable, "approval_authority_unavailable"},
		{"not resolved", fakeApprovalAuthority{err: spaces.ErrApprovalAuthorityNotResolved}, http.StatusNotFound, "approval_not_found"},
		{"not granted", fakeApprovalAuthority{authority: requested}, http.StatusConflict, "approval_not_granted"},
		{"another space", fakeApprovalAuthority{authority: elsewhere}, http.StatusNotFound, "approval_not_found"},
		{"session core unreachable", fakeApprovalAuthority{err: errors.New("session core unreachable")}, http.StatusServiceUnavailable, "approval_authority_unavailable"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			server := NewServer(nil, nil, nil, nil, "0")
			if tc.resolver != nil {
				server.SetApprovalAuthorityResolver(tc.resolver)
			}
			_, ok, rec := runProvenance(t, server)
			if ok {
				t.Fatal("a refused provenance let the standing rule through")
			}
			wantApprovalError(t, rec, tc.status, tc.code)
			if strings.Contains(rec.Body.String(), "unreachable") {
				t.Fatalf("an internal error message leaked: %s", rec.Body.String())
			}
		})
	}
}

func TestVerifyStandingRuleProvenancePassesAGrantedApprovalTheCallerCanSee(t *testing.T) {
	gin.SetMode(gin.TestMode)
	server := NewServer(nil, nil, nil, nil, "0")
	server.SetApprovalAuthorityResolver(fakeApprovalAuthority{authority: grantedProvenanceAuthority()})
	authority, ok, rec := runProvenance(t, server)
	if !ok || authority.RunID != "run-1" {
		t.Fatalf("granted approval: ok=%v authority=%+v", ok, authority)
	}
	if rec.Body.Len() != 0 {
		t.Fatalf("a passing check wrote a response: %s", rec.Body.String())
	}
}

func TestCreateApprovalGrantNamesTheApprovalBeforeAnyAuthorityLookup(t *testing.T) {
	engine := approvalHarness(t, &spaces.Repository{}, nil, gatewayContext())
	body := validCreateApprovalGrantBody()
	wantApprovalError(t, doJSON(engine, http.MethodPost, "/api/v1/internal/spaces/approval-grants", body), http.StatusBadRequest, "invalid_request")
	body["approval_id"] = "   "
	wantApprovalError(t, doJSON(engine, http.MethodPost, "/api/v1/internal/spaces/approval-grants", body), http.StatusBadRequest, "invalid_request")
}
