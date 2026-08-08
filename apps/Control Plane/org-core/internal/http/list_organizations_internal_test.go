package http

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
)

func TestListOrganizationsInternalRejectsMalformedPagination(t *testing.T) {
	gin.SetMode(gin.TestMode)

	tests := []struct {
		name  string
		query string
	}{
		{name: "non-numeric limit", query: "limit=abc"},
		{name: "zero limit", query: "limit=0"},
		{name: "negative limit", query: "limit=-1"},
		{name: "non-numeric offset", query: "offset=abc"},
		{name: "negative offset", query: "offset=-1"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			response := httptest.NewRecorder()
			ctx, _ := gin.CreateTestContext(response)
			ctx.Request = httptest.NewRequest(http.MethodGet, "/internal/orgs?"+tt.query, nil)

			// A zero-value Server has a nil orgService. If the handler reached
			// it on this input, this call would panic instead of returning a
			// clean 400 — this test's real assertion is that it never gets
			// there for malformed pagination.
			server := &Server{}
			server.listOrganizationsInternal(ctx)

			if response.Code != http.StatusBadRequest {
				t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
			}
		})
	}
}
