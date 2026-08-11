package http

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/clients"
	"github.com/I-Dacosta/AquatiqCMS/apps/conversation-core/conversation-core-go/internal/delegation"
	"github.com/gin-gonic/gin"
)

type supportPolicyStub struct {
	policy clients.SupportPolicy
	err    error
}

func (s supportPolicyStub) SupportPolicy(_ context.Context, _, _ string) (clients.SupportPolicy, error) {
	return s.policy, s.err
}

func TestSupportRecurrencePolicyFailsClosed(t *testing.T) {
	gin.SetMode(gin.TestMode)
	cases := []struct {
		name       string
		reader     supportPolicyStub
		wantStatus int
		wantCode   string
	}{
		{name: "policy unavailable", reader: supportPolicyStub{err: errors.New("org-core unavailable")}, wantStatus: http.StatusServiceUnavailable, wantCode: "support_recurrence_policy_unavailable"},
		{name: "zdr", reader: supportPolicyStub{policy: clients.SupportPolicy{ZDREnabled: true, RecurrenceAllowed: true}}, wantStatus: http.StatusPreconditionFailed, wantCode: "zdr_recurrence_forbidden"},
		{name: "missing capability", reader: supportPolicyStub{policy: clients.SupportPolicy{}}, wantStatus: http.StatusForbidden, wantCode: "support_recurrence_permission_required"},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			writer := httptest.NewRecorder()
			ctx, _ := gin.CreateTestContext(writer)
			request := httptest.NewRequest(http.MethodGet, "/api/v1/tickets/ticket-1/support-recurrence-candidates", nil)
			ctx.Request = request.WithContext(delegation.WithPrincipal(request.Context(), delegation.Principal{OrganizationID: "org-1", Role: "member"}))
			handler := &Handler{supportPolicy: tc.reader}

			if handler.requireSupportRecurrencePolicy(ctx, "org-1") {
				t.Fatal("expected policy gate to deny the request")
			}
			if writer.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d", writer.Code, tc.wantStatus)
			}
			if body := writer.Body.String(); !containsErrorCode(body, tc.wantCode) {
				t.Fatalf("body = %s, expected error code %q", body, tc.wantCode)
			}
		})
	}
}

func TestSupportRecurrencePolicyAllowsEntitledNonZDROrganization(t *testing.T) {
	gin.SetMode(gin.TestMode)
	writer := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(writer)
	request := httptest.NewRequest(http.MethodGet, "/api/v1/tickets/ticket-1/support-recurrence-candidates", nil)
	ctx.Request = request.WithContext(delegation.WithPrincipal(request.Context(), delegation.Principal{OrganizationID: "org-1", Role: "member"}))
	handler := &Handler{supportPolicy: supportPolicyStub{policy: clients.SupportPolicy{RecurrenceAllowed: true}}}

	if !handler.requireSupportRecurrencePolicy(ctx, "org-1") {
		t.Fatal("expected policy gate to allow the request")
	}
}

func containsErrorCode(body, code string) bool {
	return strings.Contains(body, `"code":"`+code+`"`)
}
