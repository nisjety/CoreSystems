package http

import (
	"github.com/gin-gonic/gin"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestAllowanceRoutesRequireDelegatedTenantAndSeparateWriteScope(t *testing.T) {
	now := time.Now().UTC()
	router := billingServiceAuthTestRouter(t, billingServiceTestRegistry("billing:account:read:self"), now)
	router.GET("/api/v1/billing/orgs/:orgId/allowances", func(c *gin.Context) { c.Status(204) })
	router.PUT("/api/v1/billing/orgs/:orgId/allowances", func(c *gin.Context) { c.Status(204) })
	router.POST("/api/v1/billing/orgs/:orgId/allowances/:metric/grants", func(c *gin.Context) { c.Status(204) })
	router.POST("/api/v1/billing/orgs/:orgId/allowances/:metric/revisions", func(c *gin.Context) { c.Status(204) })
	cases := []struct {
		method, path string
		status       int
	}{
		{http.MethodGet, "/api/v1/billing/orgs/org-1/allowances", 204},
		{http.MethodGet, "/api/v1/billing/orgs/org-2/allowances", 403},
		{http.MethodPut, "/api/v1/billing/orgs/org-1/allowances", 403},
		{http.MethodPost, "/api/v1/billing/orgs/org-1/allowances/tokens/grants", 403},
		{http.MethodPost, "/api/v1/billing/orgs/org-1/allowances/tokens/revisions", 403},
	}
	for _, tc := range cases {
		t.Run(tc.method+tc.path, func(t *testing.T) {
			r := httptest.NewRequest(tc.method, tc.path, nil)
			signBillingDelegation(r, nil, now, "allowance-nonce-"+tc.method+tc.path)
			w := httptest.NewRecorder()
			router.ServeHTTP(w, r)
			if w.Code != tc.status {
				t.Fatalf("got %d: %s", w.Code, w.Body.String())
			}
		})
	}
}
