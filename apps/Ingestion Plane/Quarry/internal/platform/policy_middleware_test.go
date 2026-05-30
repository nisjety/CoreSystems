package platform

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"
)

func TestAuthClientVerifyTokenUsesCache(t *testing.T) {
	t.Parallel()

	var calls int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/internal/sessions/verify" {
			http.NotFound(w, r)
			return
		}
		atomic.AddInt32(&calls, 1)
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"success":true,"data":{"userId":"user-1","organizationId":"org-1","workspaceId":"ws-1","role":"member","email":"user@example.com","tier":"pro"}}`))
	}))
	defer server.Close()

	client := NewAuthClientWithCache(server.URL, "internal-key", time.Minute)
	ctx := context.Background()

	first, err := client.VerifyToken(ctx, "token-1")
	if err != nil {
		t.Fatalf("first verify: %v", err)
	}
	second, err := client.VerifyToken(ctx, "token-1")
	if err != nil {
		t.Fatalf("second verify: %v", err)
	}

	if atomic.LoadInt32(&calls) != 1 {
		t.Fatalf("auth-core calls = %d, want 1", atomic.LoadInt32(&calls))
	}
	if first == second {
		t.Fatal("expected cloned principals, got same pointer")
	}
	if second.OrganizationID != "org-1" || second.Tier != "pro" {
		t.Fatalf("principal = %+v, want org-1/pro", second)
	}
}

func TestControlPlanePolicyMiddlewareEnrichesPrincipal(t *testing.T) {
	t.Parallel()

	billingServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/api/v1/billing/orgs/org-1/account" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"org_id":"org-1","plan":"enterprise","credits":88,"feature_flags":{"feature.browser":true},"entitlements":{"feature.browser":true},"quota_limits":{"async_jobs":9},"metadata":{"zdr_mode":true,"concurrency_limit":7}}`))
	}))
	defer billingServer.Close()

	orgServer := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/orgs/org-1/entitlements" {
			http.NotFound(w, r)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"organization_id":"org-1","entitlements":[{"key":"feature.research","enabled":false},{"key":"feature.browser","enabled":true}]}`))
	}))
	defer orgServer.Close()

	service := NewControlPlaneService(ControlPlaneConfig{
		BillingBaseURL: billingServer.URL,
		OrgBaseURL:     orgServer.URL,
		CacheTTL:       time.Minute,
	})

	app := fiber.New()
	app.Use(func(c *fiber.Ctx) error {
		c.Locals(PrincipalContextKey, &Principal{
			UserID:         "user-1",
			OrganizationID: "org-1",
			Tier:           "free",
		})
		return c.Next()
	})
	app.Use(ControlPlanePolicyMiddleware(ControlPlanePolicyConfig{Service: service}))
	app.Get("/", func(c *fiber.Ctx) error {
		return c.JSON(GetPrincipal(c))
	})

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	resp, err := app.Test(req, -1)
	if err != nil {
		t.Fatalf("app.Test: %v", err)
	}
	defer resp.Body.Close()

	var principal Principal
	if err := json.NewDecoder(resp.Body).Decode(&principal); err != nil {
		t.Fatalf("decode principal: %v", err)
	}

	if principal.Tier != "enterprise" || principal.Plan != "enterprise" {
		t.Fatalf("principal plan/tier = %+v, want enterprise", principal)
	}
	if !principal.PolicyResolved || !principal.ZDRMode {
		t.Fatalf("principal policy flags = %+v, want resolved+zdr", principal)
	}
	if principal.MetadataInt("concurrency_limit") != 7 || principal.QuotaInt("async_jobs") != 9 {
		t.Fatalf("principal limits = %+v, want concurrency=7 async_jobs=9", principal)
	}
	if principal.HasEntitlement("feature.browser") != true || principal.HasEntitlement("feature.research") != false {
		t.Fatalf("principal entitlements = %+v, want browser=true research=false", principal.Entitlements)
	}
}

func TestRequireEntitlementBlocksResolvedPrincipal(t *testing.T) {
	t.Parallel()

	err := RequireEntitlement("feature.research", &Principal{
		Tier:           "pro",
		PolicyResolved: true,
		Entitlements: map[string]bool{
			"feature.research": false,
		},
	}, true)
	if err == nil {
		t.Fatal("expected entitlement error, got nil")
	}

	if err := RequireEntitlement("feature.research", &Principal{
		Tier:           "pro",
		PolicyResolved: true,
		Entitlements: map[string]bool{
			"feature.research": true,
		},
	}, true); err != nil {
		t.Fatalf("expected allow, got %v", err)
	}

	if err := RequireEntitlement("feature.search", &Principal{
		Tier:           "pro",
		PolicyResolved: true,
		Entitlements:   map[string]bool{},
	}, true); err != nil {
		t.Fatalf("expected unknown feature to pass, got %v", err)
	}
}
