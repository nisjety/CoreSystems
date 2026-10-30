package http

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/social-core/internal/config"
	"github.com/gin-gonic/gin"
)

func TestResolveMetricsSnapshotOrgNeverEnumeratesAllTenants(t *testing.T) {
	cfg := &config.Config{IntegrationCoreTenantBearerToken: "test-token", IntegrationCoreTenantOrganizationID: "org-1"}
	if got, err := resolveMetricsSnapshotOrg(cfg, ""); err != nil || got != "org-1" {
		t.Fatalf("empty org resolved to %q, err=%v", got, err)
	}
	if got, err := resolveMetricsSnapshotOrg(cfg, "org-1"); err != nil || got != "org-1" {
		t.Fatalf("same org resolved to %q, err=%v", got, err)
	}
	if _, err := resolveMetricsSnapshotOrg(cfg, "org-2"); err == nil {
		t.Fatal("cross-tenant snapshot must fail")
	}
	if _, err := resolveMetricsSnapshotOrg(&config.Config{}, ""); err == nil {
		t.Fatal("unconfigured snapshot must fail")
	}
}

func TestManualMetricsSnapshotRejectsUnconfiguredAndMismatchedTenant(t *testing.T) {
	gin.SetMode(gin.TestMode)
	for _, tc := range []struct {
		cfg    *config.Config
		header string
		want   int
	}{
		{&config.Config{}, "", http.StatusServiceUnavailable},
		{&config.Config{IntegrationCoreTenantBearerToken: "test-token", IntegrationCoreTenantOrganizationID: "org-1"}, "org-2", http.StatusForbidden},
	} {
		recorder := httptest.NewRecorder()
		ctx, _ := gin.CreateTestContext(recorder)
		ctx.Request = httptest.NewRequest(http.MethodPost, "/metrics/snapshot", strings.NewReader(`{}`))
		ctx.Request.Header.Set("x-org-id", tc.header)
		NewHandler(tc.cfg, nil).SnapshotMetrics(ctx)
		if recorder.Code != tc.want {
			t.Fatalf("status=%d, want=%d", recorder.Code, tc.want)
		}
	}
}
