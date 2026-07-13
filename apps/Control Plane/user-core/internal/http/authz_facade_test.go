package http

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
)

func init() { gin.SetMode(gin.TestMode) }

// The facade handlers must reject incomplete requests (400) and fail safe when
// the repository is not wired (503) — never a panic or a silent 200. These
// paths need no DB; the grant logic itself is covered by the AclRepository
// integration tests.
func TestAuthzVisibleValidation(t *testing.T) {
	s := &Server{aclRepo: nil} // not wired

	// Missing required params → 400 (checked before the nil-repo guard).
	c, w := newGinCtx("/api/v1/internal/authz/visible?org_id=org-1")
	s.authzVisible(c)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("missing subject_id/resource_type: want 400, got %d", w.Code)
	}

	// All params present but repo unwired → 503 (fail safe, not 500/200).
	c, w = newGinCtx("/api/v1/internal/authz/visible?org_id=org-1&subject_id=u1&resource_type=document")
	s.authzVisible(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired repo: want 503, got %d", w.Code)
	}
}

func TestAuthzCheckValidation(t *testing.T) {
	s := &Server{aclRepo: nil}

	c, w := newGinCtx("/api/v1/internal/authz/check?org_id=org-1&resource_type=document")
	s.authzCheck(c)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("missing resource_id/subject_id: want 400, got %d", w.Code)
	}

	c, w = newGinCtx("/api/v1/internal/authz/check?org_id=org-1&resource_type=document&resource_id=d1&subject_id=u1")
	s.authzCheck(c)
	if w.Code != http.StatusServiceUnavailable {
		t.Fatalf("unwired repo: want 503, got %d", w.Code)
	}
}

func TestRequireServicePrincipal(t *testing.T) {
	s := &Server{}

	c, w := newGinCtx("/api/v1/internal/authz/visible")
	c.Set("auth_method", "bearer")
	s.requireServicePrincipal(c)
	if w.Code != http.StatusForbidden {
		t.Fatalf("bearer caller: want 403, got %d", w.Code)
	}

	c, w = newGinCtx("/api/v1/internal/authz/visible")
	c.Set("auth_method", "service_principal")
	s.requireServicePrincipal(c)
	if w.Code != http.StatusOK {
		t.Fatalf("service principal: want pass-through (200), got %d", w.Code)
	}
}

// Static service credentials prove only workload identity. Without a verified
// delegation binding tenant, subject, resource and actor, neither a user bearer
// nor a service principal may reach the authz facade.
func TestRequireVerifiedAuthzDelegation(t *testing.T) {
	s := &Server{}
	for _, authMethod := range []string{"bearer", "service_principal"} {
		c, w := newGinCtx("/api/v1/internal/authz/visible")
		c.Set("auth_method", authMethod)
		s.requireVerifiedAuthzDelegation(c)
		if w.Code != http.StatusForbidden {
			t.Fatalf("%s caller: want 403, got %d", authMethod, w.Code)
		}
	}

	t.Run("matching signed read delegation passes", func(t *testing.T) {
		c, w := newGinCtx("/api/v1/internal/authz/visible?org_id=org-1&subject_id=user-1&resource_type=document")
		c.Set("auth_method", "service_principal")
		c.Set("delegation_verified", true)
		c.Set("delegated_user_proof_verified", true)
		c.Set("delegation_version", "v2")
		c.Set("delegation_operation", "authz:visible")
		c.Set("delegation_resource_type", "document")
		c.Set("delegation_reason", "resolve explicit grants")
		c.Set("delegation_zdr", "true")
		c.Set("org_id", "org-1")
		c.Set("user_id", "user-1")
		s.requireVerifiedAuthzDelegation(c)
		if w.Code != http.StatusOK {
			t.Fatalf("matching signed read: want pass-through, got %d", w.Code)
		}
	})

	for _, target := range []string{
		"/api/v1/internal/authz/visible?org_id=other-org&subject_id=user-1&resource_type=document",
		"/api/v1/internal/authz/visible?org_id=org-1&subject_id=other-user&resource_type=document",
		"/api/v1/internal/authz/grants?org_id=org-1&resource_type=document&resource_id=doc-1",
	} {
		c, w := newGinCtx(target)
		c.Set("auth_method", "service_principal")
		c.Set("delegation_verified", true)
		c.Set("delegated_user_proof_verified", true)
		c.Set("delegation_version", "v2")
		c.Set("delegation_operation", "authz:visible")
		c.Set("delegation_resource_type", "document")
		c.Set("delegation_reason", "resolve explicit grants")
		c.Set("delegation_zdr", "true")
		c.Set("org_id", "org-1")
		c.Set("user_id", "user-1")
		s.requireVerifiedAuthzDelegation(c)
		if w.Code != http.StatusForbidden {
			t.Fatalf("unbounded target %q: want 403, got %d", target, w.Code)
		}
	}
}

func newGinCtx(target string) (*gin.Context, *httptest.ResponseRecorder) {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodGet, target, nil)
	return c, w
}
