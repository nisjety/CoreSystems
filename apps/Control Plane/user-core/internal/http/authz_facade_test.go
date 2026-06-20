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

// The facade must be reachable only by the internal key — a user Bearer token
// (auth_method != internal_key) must be rejected so it cannot enumerate another
// subject's grants.
func TestRequireInternalKeyOnly(t *testing.T) {
	s := &Server{}

	c, w := newGinCtx("/api/v1/internal/authz/visible")
	c.Set("auth_method", "bearer")
	s.requireInternalKeyOnly(c)
	if w.Code != http.StatusForbidden {
		t.Fatalf("bearer caller: want 403, got %d", w.Code)
	}

	c, w = newGinCtx("/api/v1/internal/authz/visible")
	c.Set("auth_method", "internal_key")
	s.requireInternalKeyOnly(c)
	if w.Code != http.StatusOK {
		t.Fatalf("internal-key caller: want pass-through (200), got %d", w.Code)
	}
}

func newGinCtx(target string) (*gin.Context, *httptest.ResponseRecorder) {
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	c.Request = httptest.NewRequest(http.MethodGet, target, nil)
	return c, w
}
