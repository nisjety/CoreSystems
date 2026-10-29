package http

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/rbac"
	"github.com/gin-gonic/gin"
)

func TestListRolesDoesNotExposeRepositoryErrors(t *testing.T) {
	gin.SetMode(gin.TestMode)
	response := httptest.NewRecorder()
	ctx, _ := gin.CreateTestContext(response)
	ctx.Request = httptest.NewRequest(http.MethodGet, "/orgs/org-1/roles", nil)
	ctx.Params = gin.Params{{Key: "id", Value: "org-1"}}

	server := &Server{rbacRepo: rbac.NewRepository(nil)}
	server.listRoles(ctx)

	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
	if strings.Contains(response.Body.String(), "repository") ||
		strings.Contains(response.Body.String(), "database") {
		t.Fatalf("internal error leaked in response: %s", response.Body.String())
	}
	if !strings.Contains(response.Body.String(), `"code":"roles_unavailable"`) {
		t.Fatalf("stable error code missing: %s", response.Body.String())
	}
}
