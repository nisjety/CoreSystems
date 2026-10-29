package http

import (
	"bytes"
	"context"
	nethttp "net/http"
	"net/http/httptest"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/database"
	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/testfixture"
)

func TestControlLifecycleBillingScopedHTTPRoutesWithPostgres(t *testing.T) {
	dsn := os.Getenv("CONTROL_LIFECYCLE_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("CONTROL_LIFECYCLE_TEST_DATABASE_URL is not set")
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	db, err := database.Connect(ctx, dsn)
	if err != nil {
		t.Fatalf("connect to disposable Postgres: %v", err)
	}
	defer db.Close()
	if err := testfixture.VerifyLifecycleMarker(
		ctx,
		db.Pool,
		dsn,
		"billing_lifecycle",
		os.Getenv("CONTROL_LIFECYCLE_FIXTURE_ID"),
	); err != nil {
		t.Fatalf("refusing unsafe lifecycle database: %v", err)
	}
	if err := database.RunMigrations(ctx, db, "../../migrations"); err != nil {
		t.Fatalf("apply billing-core migrations: %v", err)
	}

	authToken := strings.Repeat("a", 48)
	writerToken := strings.Repeat("w", 48)
	t.Setenv(serviceCredentialEnv, `[
  {"principal":"auth-core","audience":"billing-core","token":"`+authToken+`","scopes":["billing:organization:deactivate:any"]},
  {"principal":"lifecycle-writer","audience":"billing-core","token":"`+writerToken+`","scopes":["billing:account:write:any","billing:usage:write:any"]}
]`)
	repo := billing.NewRepository(db.Pool)
	service := billing.NewService(repo, nil, nil)
	server := NewServer(0, service)

	orgID := "billing-http-lifecycle"
	if err := repo.SaveAccountStateCAS(ctx, billing.Account{
		OrgID:              orgID,
		Plan:               "pro",
		SubscriptionState:  billing.SubscriptionStateActive,
		Products:           map[string]bool{},
		FeatureFlags:       map[string]bool{},
		Entitlements:       map[string]bool{"feature.integrations": true},
		QuotaLimits:        map[string]float64{"api_calls": 10_000},
		ProviderCustomerID: map[string]string{},
		Metadata:           map[string]interface{}{},
	}); err != nil {
		t.Fatalf("seed isolated billing account: %v", err)
	}

	request := func(method, path, body, principal, token string) *httptest.ResponseRecorder {
		t.Helper()
		req := httptest.NewRequest(method, path, bytes.NewBufferString(body))
		if body != "" {
			req.Header.Set("Content-Type", "application/json")
		}
		if principal != "" {
			req.Header.Set("X-Service-Id", principal)
		}
		if token != "" {
			req.Header.Set("X-Service-Token", token)
		}
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, req)
		return response
	}

	usagePath := "/api/v1/billing/orgs/" + orgID + "/usage"
	usageBody := `{"event_id":"usage_http_fixture_01","metric":"api_calls","quantity":3,"source":"lifecycle-fixture","occurred_at":"2026-07-15T10:00:00Z","metadata":{"run_id":"run-1"}}`
	for attempt := 1; attempt <= 2; attempt++ {
		response := request(nethttp.MethodPost, usagePath, usageBody, "lifecycle-writer", writerToken)
		if response.Code != nethttp.StatusAccepted {
			t.Fatalf("usage attempt %d status=%d body=%s", attempt, response.Code, response.Body.String())
		}
	}
	conflict := request(nethttp.MethodPost, usagePath,
		`{"event_id":"usage_http_fixture_01","metric":"api_calls","quantity":4,"source":"lifecycle-fixture","occurred_at":"2026-07-15T10:00:00Z","metadata":{"run_id":"run-1"}}`,
		"lifecycle-writer", writerToken)
	if conflict.Code != nethttp.StatusConflict {
		t.Fatalf("usage conflict status=%d body=%s", conflict.Code, conflict.Body.String())
	}
	var usageRows, usageJobs int
	if err := db.Pool.QueryRow(ctx, `
SELECT
  (SELECT COUNT(*)::INT FROM billing_usage_events WHERE event_id = 'usage_http_fixture_01'),
  (SELECT COUNT(*)::INT FROM billing_retry_jobs WHERE dedupe_key = 'lago_usage_report:usage_http_fixture_01')`).Scan(
		&usageRows, &usageJobs,
	); err != nil {
		t.Fatalf("read HTTP usage idempotency rows: %v", err)
	}
	if usageRows != 1 || usageJobs != 1 {
		t.Fatalf("HTTP usage rows=%d jobs=%d; want 1/1", usageRows, usageJobs)
	}

	deactivatePath := "/api/v1/billing/orgs/" + orgID + "/deactivate"
	if response := request(nethttp.MethodPost, deactivatePath, `{"reason":"organization_deleted"}`, "", ""); response.Code != nethttp.StatusUnauthorized {
		t.Fatalf("missing principal status=%d body=%s", response.Code, response.Body.String())
	}
	if response := request(nethttp.MethodPost, deactivatePath, `{"reason":"organization_deleted"}`, "lifecycle-writer", writerToken); response.Code != nethttp.StatusForbidden {
		t.Fatalf("wrong-scope principal status=%d body=%s", response.Code, response.Body.String())
	}
	legacy := httptest.NewRequest(nethttp.MethodPost, deactivatePath, bytes.NewBufferString(`{"reason":"organization_deleted"}`))
	legacy.Header.Set("Content-Type", "application/json")
	legacy.Header.Set("X-Internal-Api-Key", authToken)
	legacyResponse := httptest.NewRecorder()
	server.router.ServeHTTP(legacyResponse, legacy)
	if legacyResponse.Code != nethttp.StatusUnauthorized {
		t.Fatalf("legacy key status=%d body=%s", legacyResponse.Code, legacyResponse.Body.String())
	}

	for attempt := 1; attempt <= 2; attempt++ {
		response := request(nethttp.MethodPost, deactivatePath,
			`{"reason":"organization_deleted"}`, "auth-core", authToken)
		if response.Code != nethttp.StatusOK {
			t.Fatalf("deactivation attempt %d status=%d body=%s", attempt, response.Code, response.Body.String())
		}
	}

	// A delayed account mutation passes the scoped route gate but is rejected by
	// the repository tombstone, proving middleware and persistence fail closed
	// together rather than merely hiding the route.
	accountPath := "/api/v1/billing/orgs/" + orgID + "/account"
	delayed := request(nethttp.MethodPut, accountPath,
		`{"plan":"enterprise","subscription_state":"active"}`,
		"lifecycle-writer", writerToken)
	if delayed.Code != nethttp.StatusInternalServerError {
		t.Fatalf("post-delete account mutation status=%d body=%s", delayed.Code, delayed.Body.String())
	}

	account, err := repo.GetAccount(ctx, orgID)
	if err != nil {
		t.Fatalf("read final billing account: %v", err)
	}
	var tombstones int
	if err := db.Pool.QueryRow(ctx,
		`SELECT COUNT(*)::INT FROM billing_organization_tombstones WHERE org_id = $1`, orgID,
	).Scan(&tombstones); err != nil {
		t.Fatalf("count final billing tombstone: %v", err)
	}
	if account.Plan != "pro" || account.SubscriptionState != billing.SubscriptionStateCanceled || tombstones != 1 {
		t.Fatalf("final billing lifecycle state plan=%q state=%q tombstones=%d",
			account.Plan, account.SubscriptionState, tombstones)
	}
}
