package http

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

type entitlementCheckerStub struct {
	allowed bool
	err     error
}

func (s entitlementCheckerStub) Allowed(_ context.Context, _, _ string) (bool, error) {
	return s.allowed, s.err
}

func TestRequireLeadsEntitlementFailsClosed(t *testing.T) {
	gin.SetMode(gin.TestMode)
	cases := []struct {
		name       string
		checker    entitlementCheckerStub
		wantStatus int
		wantCode   string
	}{
		{name: "denied", checker: entitlementCheckerStub{allowed: false}, wantStatus: http.StatusPaymentRequired, wantCode: "entitlement_required"},
		{name: "unavailable", checker: entitlementCheckerStub{err: errors.New("billing unavailable")}, wantStatus: http.StatusServiceUnavailable, wantCode: "entitlement_check_unavailable"},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			writer := httptest.NewRecorder()
			ctx, _ := gin.CreateTestContext(writer)
			ctx.Request = httptest.NewRequest(http.MethodPost, "/api/v1/leads/build_list", nil)
			handler := &Handler{entitlements: tc.checker}

			if handler.requireLeadsEntitlement(ctx, "org-1") {
				t.Fatal("expected entitlement gate to deny the request")
			}
			if writer.Code != tc.wantStatus {
				t.Fatalf("status = %d, want %d", writer.Code, tc.wantStatus)
			}
			if got := ctx.Errors.String(); got != "" {
				t.Fatalf("unexpected Gin errors: %s", got)
			}
			if body := writer.Body.String(); !containsJSONCode(body, tc.wantCode) {
				t.Fatalf("body = %s, expected error code %q", body, tc.wantCode)
			}
		})
	}
}

func TestRequireLeadsEntitlementAllowsGrantedFeature(t *testing.T) {
	gin.SetMode(gin.TestMode)
	writer := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(writer)
	ctx.Request = httptest.NewRequest(http.MethodPost, "/api/v1/leads/build_list", nil)
	handler := &Handler{entitlements: entitlementCheckerStub{allowed: true}}

	if !handler.requireLeadsEntitlement(ctx, "org-1") {
		t.Fatal("expected granted entitlement to proceed")
	}
}

func containsJSONCode(body, code string) bool {
	return strings.Contains(body, `"code":"`+code+`"`)
}
