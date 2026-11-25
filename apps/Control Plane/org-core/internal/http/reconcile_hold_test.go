package http

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/retention"
	"github.com/gin-gonic/gin"
)

// reconcileAuditCall is one refusal audit the reconcile route wrote.
type reconcileAuditCall struct {
	orgID       string
	actor       retention.Actor
	op          string
	orgHolds    int
	threadHolds int
}

// reconcileRefusalRecorder is the retention auditor the org service writes refusals to.
type reconcileRefusalRecorder struct {
	calls []reconcileAuditCall
}

func (r *reconcileRefusalRecorder) Holds(context.Context, string) (retention.HoldList, error) {
	return retention.HoldList{}, nil
}

func (r *reconcileRefusalRecorder) AuditRefusedErasure(_ context.Context, orgID string, actor retention.Actor, op string, orgHolds, threadHolds int) error {
	r.calls = append(r.calls, reconcileAuditCall{orgID, actor, op, orgHolds, threadHolds})
	return nil
}

// reconcileRouteFixture is a server whose org service records refusals to the recorder.
func reconcileRouteFixture(t *testing.T) (*Server, *reconcileRefusalRecorder, *gin.Context, *httptest.ResponseRecorder) {
	t.Helper()
	gin.SetMode(gin.TestMode)
	recorder := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(recorder)
	c.Request = httptest.NewRequest(http.MethodPost, "/internal/orgs/org-held/reconcile-delete", nil)
	auditor := &reconcileRefusalRecorder{}
	svc := &orgcore.Service{}
	svc.SetErasureRefusalAuditor(auditor)
	return &Server{orgService: svc}, auditor, c, recorder
}

// reconcileErrorBody decodes the typed envelope a refusal writes.
type reconcileErrorBody struct {
	Error struct {
		Code    string         `json:"code"`
		Details map[string]any `json:"details"`
	} `json:"error"`
}

// F-f10-06 (reconcile route): a held organisation is refused 409 erasure_blocked_legal_hold
// with the counts, the refusal is audited once as a hard delete under the projection actor,
// and nothing is erased.
func TestReconcileRouteAnswersAHeldRefusalAndAuditsIt(t *testing.T) {
	server, auditor, c, recorder := reconcileRouteFixture(t)
	err := fmt.Errorf("reconcile: %w", &orgcore.ErasureHeldError{OrgHolds: 1, ThreadHolds: 2})
	if !server.refuseHeldReconcile(c, "org-held", err) {
		t.Fatal("held refusal was not recognised")
	}
	if recorder.Code != http.StatusConflict {
		t.Fatalf("status = %d, want 409", recorder.Code)
	}
	var body reconcileErrorBody
	if decodeErr := json.Unmarshal(recorder.Body.Bytes(), &body); decodeErr != nil {
		t.Fatalf("decode refusal: %v: %s", decodeErr, recorder.Body.String())
	}
	if body.Error.Code != codeErasureBlockedLegalHold {
		t.Fatalf("code = %q, want %q", body.Error.Code, codeErasureBlockedLegalHold)
	}
	if body.Error.Details["org_holds"] != float64(1) || body.Error.Details["thread_holds"] != float64(2) || body.Error.Details["retryable"] != false {
		t.Fatalf("details = %+v, want the counts and retryable false", body.Error.Details)
	}
	want := reconcileAuditCall{
		orgID: "org-held",
		actor: retention.Actor{ID: orgcore.GDPRReconcileActorID, Role: "system"},
		op:    retention.ErasureOpHardDelete, orgHolds: 1, threadHolds: 2,
	}
	if len(auditor.calls) != 1 || auditor.calls[0] != want {
		t.Fatalf("audit calls = %+v, want [%+v]", auditor.calls, want)
	}
}

// Holds that cannot be read are 503 erasure_hold_check_unavailable and retryable, and write
// no audit row, as the erasure routes do.
func TestReconcileRouteAnswersAnUnreadableHoldCheckRetryably(t *testing.T) {
	server, auditor, c, recorder := reconcileRouteFixture(t)
	err := fmt.Errorf("reconcile: %w", orgcore.ErrErasureHoldCheckUnavailable)
	if !server.refuseHeldReconcile(c, "org-held", err) {
		t.Fatal("unavailable refusal was not recognised")
	}
	if recorder.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503", recorder.Code)
	}
	var body reconcileErrorBody
	if decodeErr := json.Unmarshal(recorder.Body.Bytes(), &body); decodeErr != nil {
		t.Fatalf("decode refusal: %v: %s", decodeErr, recorder.Body.String())
	}
	if body.Error.Code != codeErasureHoldCheckUnavailable || body.Error.Details["retryable"] != true {
		t.Fatalf("refusal = %+v, want erasure_hold_check_unavailable and retryable", body.Error)
	}
	if len(auditor.calls) != 0 {
		t.Fatalf("unavailable refusal wrote %d audit rows, want none", len(auditor.calls))
	}
}

// Any other reconcile error is not a hold refusal: the handler's existing mapping answers it,
// and this helper writes nothing and audits nothing.
func TestReconcileRouteLeavesOtherErrorsToTheExistingMapping(t *testing.T) {
	server, auditor, c, recorder := reconcileRouteFixture(t)
	if server.refuseHeldReconcile(c, "org-held", orgcore.ErrProjectionConflict) {
		t.Fatal("projection conflict was treated as a hold refusal")
	}
	if recorder.Body.Len() != 0 || len(auditor.calls) != 0 {
		t.Fatalf("non-hold error wrote body=%q audits=%d, want neither", recorder.Body.String(), len(auditor.calls))
	}
}
