package http

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
)

func init() { gin.SetMode(gin.TestMode) }

// newGDPRTestContext builds a gin context wired into a recorder with the auth
// context values normally set by authContextMiddleware.
func newGDPRTestContext(t *testing.T, method, path, body, userID, role string) (*gin.Context, *httptest.ResponseRecorder) {
	t.Helper()
	w := httptest.NewRecorder()
	c, _ := gin.CreateTestContext(w)
	req := httptest.NewRequest(method, path, bytes.NewBufferString(body))
	req.Header.Set("Content-Type", "application/json")
	c.Request = req
	if userID != "" {
		c.Set("user_id", userID)
	}
	if role != "" {
		c.Set("user_role", role)
	}
	return c, w
}

// TestHardEraseRequiresConfirm is the core safety guard: an authorized
// hard-erase request WITHOUT confirm:true must be rejected (400) and must NOT
// reach the (nil) service — proving the confirm gate fires before any work.
func TestHardEraseRequiresConfirm(t *testing.T) {
	s := &Server{} // userService nil — a service call would panic, so reaching it fails loudly.

	// Self-authorized (caller id == target id), confirm omitted.
	c, w := newGDPRTestContext(t, http.MethodDelete, "/api/v1/users/u_123/gdpr/erase", `{"confirm":false}`, "u_123", "")
	c.Params = gin.Params{{Key: "id", Value: "u_123"}}

	s.hardEraseUser(c)

	if w.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 when confirm omitted, got %d (body=%s)", w.Code, w.Body.String())
	}
	if !strings.Contains(w.Body.String(), "irreversible") {
		t.Errorf("expected irreversible-confirm error, got %s", w.Body.String())
	}
}

// TestHardEraseRejectsNonSelfNonAdmin proves the admin/self gate: a caller who
// is neither the subject nor an admin gets 403 before any service call.
func TestHardEraseRejectsNonSelfNonAdmin(t *testing.T) {
	s := &Server{}

	c, w := newGDPRTestContext(t, http.MethodDelete, "/api/v1/users/u_target/gdpr/erase", `{"confirm":true}`, "u_other", "member")
	c.Params = gin.Params{{Key: "id", Value: "u_target"}}

	s.hardEraseUser(c)

	if w.Code != http.StatusForbidden {
		t.Fatalf("expected 403 for non-self non-admin, got %d (body=%s)", w.Code, w.Body.String())
	}
}

// TestHardEraseRequiresAuth proves the unauthenticated case returns 401.
func TestHardEraseRequiresAuth(t *testing.T) {
	s := &Server{}

	c, w := newGDPRTestContext(t, http.MethodDelete, "/api/v1/users/u_target/gdpr/erase", `{"confirm":true}`, "", "")
	c.Params = gin.Params{{Key: "id", Value: "u_target"}}

	s.hardEraseUser(c)

	if w.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401 unauthenticated, got %d (body=%s)", w.Code, w.Body.String())
	}
}

// TestActorRole covers the audit actor_role labelling.
func TestActorRole(t *testing.T) {
	if got := actorRole(true, false); got != "admin" {
		t.Errorf("admin actor = %q, want admin", got)
	}
	if got := actorRole(false, true); got != "self" {
		t.Errorf("self actor = %q, want self", got)
	}
	if got := actorRole(false, false); got != "user" {
		t.Errorf("default actor = %q, want user", got)
	}
}

// TestErasureProcCallsAreParameterized guards against SQL injection: the GDPR
// proc invocations in the users package must bind the user id as a parameter
// ($1), never interpolate it into the SQL string.
func TestErasureProcCallsAreParameterized(t *testing.T) {
	b, err := os.ReadFile("../users/gdpr.go")
	if err != nil {
		t.Fatalf("read gdpr.go: %v", err)
	}
	src := string(b)

	for _, want := range []string{
		"gdpr_hard_delete_user($1)",
		"gdpr_anonymize_user($1)",
	} {
		if !strings.Contains(src, want) {
			t.Errorf("gdpr.go missing parameterized proc call %q", want)
		}
	}
	for _, bad := range []string{
		"fmt.Sprintf(\"SELECT gdpr_hard_delete_user",
		"fmt.Sprintf(\"SELECT gdpr_anonymize_user",
		"gdpr_hard_delete_user(' +",
	} {
		if strings.Contains(src, bad) {
			t.Errorf("gdpr.go contains non-parameterized proc call %q (SQL injection risk)", bad)
		}
	}
}
