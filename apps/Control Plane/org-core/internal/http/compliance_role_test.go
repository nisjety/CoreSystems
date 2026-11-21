package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

// O-21: the compliance role is accepted as a member role only while
// ORG_COMPLIANCE_ROLE_ENABLED is the literal "true".

func TestComplianceRoleSwitchIsReadStrictlyAtBoot(t *testing.T) {
	cases := map[string]bool{"true": true, "TRUE": false, "1": false, "yes": false, "": false}
	for value, want := range cases {
		t.Setenv("ORG_COMPLIANCE_ROLE_ENABLED", value)
		if got := NewServer(0, nil, nil, "", "").complianceRoleOn; got != want {
			t.Fatalf("ORG_COMPLIANCE_ROLE_ENABLED=%q: complianceRoleOn=%v, want %v", value, got, want)
		}
	}
}

// reconcileStatus posts a membership projection to the reconcile handler. The
// service is nil, so a role that passes validation reaches it and the recovery
// middleware answers 500; only a refused role answers 400.
func reconcileStatus(t *testing.T, server *Server, role string) (int, string) {
	t.Helper()
	router := gin.New()
	router.Use(gin.Recovery())
	router.POST("/internal/orgs/:orgId/members/reconcile", server.reconcileOrganizationMember)
	body := `{"userId":"u-1","role":"` + role + `","action":"upsert","revision":1}`
	request := httptest.NewRequest(http.MethodPost, "/internal/orgs/org-1/members/reconcile", bytes.NewReader([]byte(body)))
	request.Header.Set("Content-Type", "application/json")
	response := httptest.NewRecorder()
	router.ServeHTTP(response, request)
	return response.Code, response.Body.String()
}

func TestReconcileRefusesComplianceWhileTheSwitchIsOff(t *testing.T) {
	server := &Server{}
	code, body := reconcileStatus(t, server, "compliance")
	if code != http.StatusBadRequest || strings.Contains(body, "compliance") {
		t.Fatalf("off: compliance must be refused with the unchanged message: status=%d body=%s", code, body)
	}
}

func TestReconcileAcceptsComplianceOnlyWhileTheSwitchIsOn(t *testing.T) {
	server := &Server{complianceRoleOn: true}
	if code, body := reconcileStatus(t, server, "compliance"); code == http.StatusBadRequest {
		t.Fatalf("on: compliance must pass role validation: status=%d body=%s", code, body)
	}
	if code, body := reconcileStatus(t, server, "auditor"); code != http.StatusBadRequest || !strings.Contains(body, "compliance") {
		t.Fatalf("on: an unknown role is still refused, naming compliance: status=%d body=%s", code, body)
	}
	// The four base tiers pass either way.
	for _, role := range []string{"owner", "admin", "member", "viewer"} {
		if code, body := reconcileStatus(t, &Server{}, role); code == http.StatusBadRequest {
			t.Fatalf("off: %s must stay valid: status=%d body=%s", role, code, body)
		}
	}
}

func TestInviteRefusesComplianceWhileTheSwitchIsOff(t *testing.T) {
	invite := func(server *Server, role string) (int, string) {
		router := gin.New()
		router.Use(gin.Recovery())
		router.POST("/orgs/:id/members/invite", server.inviteMember)
		body := `{"email":"a@example.com","role":"` + role + `"}`
		request := httptest.NewRequest(http.MethodPost, "/orgs/org-1/members/invite", bytes.NewReader([]byte(body)))
		request.Header.Set("Content-Type", "application/json")
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		return response.Code, response.Body.String()
	}
	if code, body := invite(&Server{}, "compliance"); code != http.StatusBadRequest || strings.Contains(body, "compliance") {
		t.Fatalf("off: status=%d body=%s", code, body)
	}
	if code, body := invite(&Server{complianceRoleOn: true}, "compliance"); code == http.StatusBadRequest {
		t.Fatalf("on: compliance must pass: status=%d body=%s", code, body)
	}
}
