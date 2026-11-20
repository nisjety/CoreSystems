package http

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/I-Dacosta/AquatiqCMS/apps/user-service-go/internal/spaces"
)

// D21h: POST /api/v1/internal/spaces/browser-run-stop-decision (pure tier).
// The 200 leg needs a database (evidence resolution) and is owed to the
// TEST_DATABASE_URL Live suite; the issuer itself is covered in the spaces
// package (space_browser_run_stop_decision_test.go).

const browserRunStopPath = "/api/v1/internal/spaces/browser-run-stop-decision"

func validBrowserRunStopBody() map[string]any {
	return map[string]any{"space_ref": "room-1", "run_id": "run-browser-1", "idempotency_key": "stop-1"}
}

func TestBrowserRunStopRouteIsRegisteredOnceUnderTheIssueScope(t *testing.T) {
	gin.SetMode(gin.TestMode)
	counts := routeCounts(NewServer(nil, nil, nil, nil, "0").router.Routes())
	if counts["POST "+browserRunStopPath] != 1 {
		t.Fatalf("browser-run-stop-decision registered %d times, want 1", counts["POST "+browserRunStopPath])
	}
	scope := serviceScopeForRequest(httptest.NewRequest(http.MethodPost, browserRunStopPath, nil))
	if scope != "spaces:issue" {
		t.Fatalf("scope = %q, want spaces:issue", scope)
	}
	if !serviceScopeRequiresVerifiedDelegation(scope) {
		t.Fatal("spaces:issue must require a verified gateway delegation")
	}
	if got := serviceScopeForRequest(httptest.NewRequest(http.MethodGet, browserRunStopPath, nil)); got == "spaces:issue" {
		t.Fatalf("a GET of the issuer path must not map to spaces:issue, got %q", got)
	}
}

func TestBrowserRunStopRouteRequiresAVerifiedGatewayDelegation(t *testing.T) {
	for name, ctx := range map[string]map[string]any{
		"no auth":              {},
		"user session":         {"auth_method": "user_session", "org_id": "org-1", "user_id": "user-1"},
		"other service":        {"auth_method": "service_principal", "service_id": "documents-api", "delegation_verified": true},
		"gateway, unverified":  {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal, "delegation_verified": false},
		"gateway without flag": {"auth_method": "service_principal", "service_id": verevonGatewayPrincipal},
	} {
		t.Run(name, func(t *testing.T) {
			engine := spaceIssuerHarness(t, &spaces.Repository{}, ctx)
			if rec := postJSON(engine, browserRunStopPath, validBrowserRunStopBody()); rec.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403 (%s)", rec.Code, rec.Body.String())
			}
		})
	}
}

func TestBrowserRunStopRouteFailsClosedWithoutARepository(t *testing.T) {
	engine := spaceIssuerHarness(t, nil, gatewayContext())
	if rec := postJSON(engine, browserRunStopPath, validBrowserRunStopBody()); rec.Code != http.StatusServiceUnavailable {
		t.Fatalf("status = %d, want 503 (%s)", rec.Code, rec.Body.String())
	}
}

func TestBrowserRunStopRouteRejectsMalformedRequestsBeforeReadingAnyEvidence(t *testing.T) {
	engine := spaceIssuerHarness(t, &spaces.Repository{}, gatewayContext())
	tests := []struct {
		name   string
		mutate func(map[string]any)
	}{
		{"missing space_ref", func(b map[string]any) { delete(b, "space_ref") }},
		{"missing run_id", func(b map[string]any) { delete(b, "run_id") }},
		{"missing idempotency_key", func(b map[string]any) { delete(b, "idempotency_key") }},
		{"padded run_id", func(b map[string]any) { b["run_id"] = " run-browser-1" }},
		{"run_id too long", func(b map[string]any) { b["run_id"] = strings.Repeat("r", 129) }},
		{"actor smuggled in", func(b map[string]any) { b["user_id"] = "user-9" }},
		{"org smuggled in", func(b map[string]any) { b["org_id"] = "org-9" }},
		{"role smuggled in", func(b map[string]any) { b["role"] = "owner" }},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			body := validBrowserRunStopBody()
			test.mutate(body)
			rec := postJSON(engine, browserRunStopPath, body)
			if rec.Code != http.StatusBadRequest || !strings.Contains(rec.Body.String(), spaceBrowserRunStopInvalidMessage) {
				t.Fatalf("status = %d, want 400 with the fixed message (%s)", rec.Code, rec.Body.String())
			}
		})
	}
	// A well-formed body passes the shape check and reaches evidence
	// resolution, which fails closed against the database-less repository with
	// the fixed refusal (as run-control-decision does), never a 400 or a 200.
	if rec := postJSON(engine, browserRunStopPath, validBrowserRunStopBody()); rec.Code != http.StatusForbidden ||
		!strings.Contains(rec.Body.String(), spaceBrowserRunStopDeniedMessage) {
		t.Fatalf("a valid body must reach evidence resolution: status = %d (%s)", rec.Code, rec.Body.String())
	}
}
