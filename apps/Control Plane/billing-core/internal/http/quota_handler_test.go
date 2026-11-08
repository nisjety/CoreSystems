package http

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/I-Dacosta/AquatiqCMS/apps/billing-core/internal/billing"
	"github.com/gin-gonic/gin"
)

const (
	quarryEdgeTestToken   = "fedcba9876543210fedcba9876543210"
	otherMachineTestToken = "0123456789abcdeffedcba9876543210"
	quarryLimitsTestPath  = "/api/v1/billing/orgs/org-1/quarry/limits"
)

type fakeQuarryLimitsReader struct {
	calls    []string
	snapshot billing.QuarryLimits
	err      error
	sameOrg  bool
}

func (f *fakeQuarryLimitsReader) QuarryLimits(_ context.Context, orgID string, now time.Time) (billing.QuarryLimits, error) {
	f.calls = append(f.calls, orgID)
	if f.err != nil {
		return billing.QuarryLimits{}, f.err
	}
	snapshot := f.snapshot
	if f.sameOrg {
		snapshot.OrgID = orgID
	}
	snapshot.AsOf = now
	return snapshot, nil
}

func int64Ptr(value int64) *int64 { return &value }

func sampleQuarryLimits() billing.QuarryLimits {
	return billing.QuarryLimits{
		Account: billing.QuarryAccountPresent,
		Plan:    "pro",
		Allowances: map[string]billing.QuarryAllowance{
			billing.QuarryMetricCrawlPage: {
				Metric: billing.QuarryMetricCrawlPage, Configured: true, Source: billing.QuarryAllowanceSourcePolicy,
				Active: true, Limit: int64Ptr(25), Used: 5, Remaining: int64Ptr(20),
			},
			billing.QuarryMetricBatchURL:           billing.QuarryAllowanceUnconfigured(billing.QuarryMetricBatchURL),
			billing.QuarryMetricBrowserSecondAgent: billing.QuarryAllowanceUnconfigured(billing.QuarryMetricBrowserSecondAgent),
		},
		Caps: billing.QuarryPlanCaps{AgentRuns: int64Ptr(8)},
	}
}

// quarryLimitsRegistry: the gateway (delegated :self scopes), quarry-edge as
// R-13 PR 2 registers it, and another machine that also holds the quota scope.
func quarryLimitsRegistry() string {
	return `[` +
		`{"principal":"verevon-gateway","audience":"billing-core","token":"` + billingServiceTestToken + `","scopes":["billing:account:read:self","billing:entitlement:read:self","billing:quota:read:self","billing:checkout:create:self","billing:checkout:confirm:self"]},` +
		`{"principal":"auth-core","audience":"billing-core","token":"0123456789abcdef0123456789abcdef","scopes":["billing:organization:deactivate:any"]},` +
		`{"principal":"quarry-edge","audience":"billing-core","token":"` + quarryEdgeTestToken + `","scopes":["billing:quota:read:any","billing:entitlement:read:any"]},` +
		`{"principal":"other-machine","audience":"billing-core","token":"` + otherMachineTestToken + `","scopes":["billing:quota:read:any"]}` +
		`]`
}

func quarryLimitsTestServer(t *testing.T, reader quarryLimitsReader, now time.Time) *gin.Engine {
	t.Helper()
	credentials, err := parseServiceCredentials(quarryLimitsRegistry())
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	server := &Server{router: gin.New(), quarryLimits: reader}
	server.router.Use(serviceAuthMiddleware(credentials, func() time.Time { return now }))
	v1 := server.router.Group("/api/v1/billing")
	server.registerQuarryLimitsRoute(v1)
	stub := func(c *gin.Context) { c.Status(http.StatusNoContent) }
	v1.GET("/orgs/:orgId/quotas/:metric", stub)
	v1.GET("/orgs/:orgId/account", stub)
	v1.POST("/orgs/:orgId/usage", stub)
	v1.PUT("/orgs/:orgId/allowances", stub)
	v1.GET("/orgs/:orgId/entitlements/:feature", stub)
	return server.router
}

func quarryEdgeRequest(method, path, actingOrg string) *http.Request {
	request := httptest.NewRequest(method, path, nil)
	request.Header.Set("X-Service-Id", quarryEdgeServicePrincipal)
	request.Header.Set("X-Service-Token", quarryEdgeTestToken)
	if actingOrg != "" {
		request.Header.Set(quarryLimitsOrgHeader, actingOrg)
	}
	return request
}

func decodeErrorCode(t *testing.T, body []byte) string {
	t.Helper()
	var envelope struct {
		Error struct {
			Code    string `json:"code"`
			Message string `json:"message"`
		} `json:"error"`
	}
	if err := json.Unmarshal(body, &envelope); err != nil {
		t.Fatalf("decode error envelope: %v body=%s", err, body)
	}
	return envelope.Error.Code
}

func TestQuarryLimitsServesTheOrgBoundSnapshotWithShortCacheControl(t *testing.T) {
	now := time.Date(2026, 11, 3, 10, 0, 0, 0, time.UTC)
	reader := &fakeQuarryLimitsReader{snapshot: sampleQuarryLimits(), sameOrg: true}
	router := quarryLimitsTestServer(t, reader, now)

	response := httptest.NewRecorder()
	router.ServeHTTP(response, quarryEdgeRequest(http.MethodGet, quarryLimitsTestPath, "org-1"))
	if response.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
	if got := response.Header().Get("Cache-Control"); got != "private, max-age=30" {
		t.Fatalf("Cache-Control=%q", got)
	}
	if len(reader.calls) != 1 || reader.calls[0] != "org-1" {
		t.Fatalf("reader calls=%v want [org-1]", reader.calls)
	}
	var envelope struct {
		Data struct {
			OrgID      string                     `json:"org_id"`
			Account    string                     `json:"account"`
			Plan       string                     `json:"plan"`
			Allowances map[string]json.RawMessage `json:"allowances"`
			Caps       map[string]*int64          `json:"caps"`
		} `json:"data"`
	}
	if err := json.Unmarshal(response.Body.Bytes(), &envelope); err != nil {
		t.Fatalf("decode: %v", err)
	}
	if envelope.Data.OrgID != "org-1" || envelope.Data.Plan != "pro" || envelope.Data.Account != "present" {
		t.Fatalf("data=%+v", envelope.Data)
	}
	for _, metric := range billing.QuarryLimitMetrics {
		if _, ok := envelope.Data.Allowances[metric]; !ok {
			t.Fatalf("allowance %s missing: %s", metric, response.Body.String())
		}
	}
	var batch map[string]any
	if err := json.Unmarshal(envelope.Data.Allowances[billing.QuarryMetricBatchURL], &batch); err != nil {
		t.Fatalf("decode batch: %v", err)
	}
	if batch["configured"] != false || batch["remaining"] != nil {
		t.Fatalf("unconfigured must be configured=false remaining=null, got %v", batch)
	}
	for _, key := range []string{"agent_runs", "interactive_runs", "render_permits", "crawl_jobs", "job_backlog", "rate_multiplier"} {
		if _, ok := envelope.Data.Caps[key]; !ok {
			t.Fatalf("cap %s missing from the contract: %s", key, response.Body.String())
		}
	}
	if envelope.Data.Caps["agent_runs"] == nil || *envelope.Data.Caps["agent_runs"] != 8 || envelope.Data.Caps["crawl_jobs"] != nil {
		t.Fatalf("caps=%v", envelope.Data.Caps)
	}
}

func TestQuarryLimitsRefusesAForeignOrgBeforeReading(t *testing.T) {
	now := time.Now().UTC()
	cases := []struct {
		name   string
		mutate func(*http.Request)
		status int
		code   string
	}{
		{name: "other org in the binding", mutate: func(r *http.Request) { r.Header.Set(quarryLimitsOrgHeader, "org-2") }, status: http.StatusForbidden, code: "foreign_org"},
		{name: "binding differs by case", mutate: func(r *http.Request) { r.Header.Set(quarryLimitsOrgHeader, "ORG-1") }, status: http.StatusForbidden, code: "foreign_org"},
		{name: "binding missing", mutate: func(r *http.Request) { r.Header.Del(quarryLimitsOrgHeader) }, status: http.StatusBadRequest, code: "org_binding_required"},
		{name: "binding repeated", mutate: func(r *http.Request) { r.Header.Add(quarryLimitsOrgHeader, "org-2") }, status: http.StatusBadRequest, code: "org_binding_required"},
		{name: "binding padded", mutate: func(r *http.Request) { r.Header.Set(quarryLimitsOrgHeader, " org-1") }, status: http.StatusBadRequest, code: "org_binding_required"},
		{name: "X-Org-Id is not a binding", mutate: func(r *http.Request) {
			r.Header.Del(quarryLimitsOrgHeader)
			r.Header.Set("X-Org-Id", "org-1")
		}, status: http.StatusBadRequest, code: "org_binding_required"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			reader := &fakeQuarryLimitsReader{snapshot: sampleQuarryLimits(), sameOrg: true}
			router := quarryLimitsTestServer(t, reader, now)
			request := quarryEdgeRequest(http.MethodGet, quarryLimitsTestPath, "org-1")
			tc.mutate(request)
			response := httptest.NewRecorder()
			router.ServeHTTP(response, request)
			if response.Code != tc.status {
				t.Fatalf("status=%d want=%d body=%s", response.Code, tc.status, response.Body.String())
			}
			if code := decodeErrorCode(t, response.Body.Bytes()); code != tc.code {
				t.Fatalf("code=%q want=%q", code, tc.code)
			}
			if got := response.Header().Get("Cache-Control"); got != "no-store" {
				t.Fatalf("error Cache-Control=%q want no-store", got)
			}
			if len(reader.calls) != 0 {
				t.Fatalf("a refused request must not read billing state; calls=%v", reader.calls)
			}
		})
	}
}

func TestQuarryLimitsNeverServesASnapshotForAnotherOrg(t *testing.T) {
	snapshot := sampleQuarryLimits()
	snapshot.OrgID = "org-2"
	reader := &fakeQuarryLimitsReader{snapshot: snapshot}
	router := quarryLimitsTestServer(t, reader, time.Now().UTC())
	response := httptest.NewRecorder()
	router.ServeHTTP(response, quarryEdgeRequest(http.MethodGet, quarryLimitsTestPath, "org-1"))
	if response.Code != http.StatusInternalServerError {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
	if strings.Contains(response.Body.String(), "org-2") || strings.Contains(response.Body.String(), "allowances") {
		t.Fatalf("the other organization's answer leaked: %s", response.Body.String())
	}
}

func TestQuarryLimitsRouteIsQuarryEdgeOnlyAndReadOnly(t *testing.T) {
	now := time.Date(2026, 11, 3, 10, 0, 0, 0, time.UTC)
	reader := &fakeQuarryLimitsReader{snapshot: sampleQuarryLimits(), sameOrg: true}
	router := quarryLimitsTestServer(t, reader, now)

	t.Run("gateway delegation cannot reach the route", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, quarryLimitsTestPath, nil)
		signBillingDelegation(request, nil, now, "quarry-limits-gateway-nonce")
		request.Header.Set(quarryLimitsOrgHeader, "org-1")
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
	})

	t.Run("another machine with the quota scope is refused", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, quarryLimitsTestPath, nil)
		request.Header.Set("X-Service-Id", "other-machine")
		request.Header.Set("X-Service-Token", otherMachineTestToken)
		request.Header.Set(quarryLimitsOrgHeader, "org-1")
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden || decodeErrorCode(t, response.Body.Bytes()) != "principal_not_allowed" {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
	})

	t.Run("quarry-edge token for another principal is refused", func(t *testing.T) {
		request := httptest.NewRequest(http.MethodGet, quarryLimitsTestPath, nil)
		request.Header.Set("X-Service-Id", "other-machine")
		request.Header.Set("X-Service-Token", quarryEdgeTestToken)
		request.Header.Set(quarryLimitsOrgHeader, "org-1")
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
		}
	})

	cases := []struct {
		name   string
		method string
		path   string
		token  string
		status int
	}{
		{name: "missing token", method: http.MethodGet, path: quarryLimitsTestPath, token: "", status: http.StatusUnauthorized},
		{name: "wrong token", method: http.MethodGet, path: quarryLimitsTestPath, token: otherMachineTestToken, status: http.StatusUnauthorized},
		{name: "write verb on the route", method: http.MethodPost, path: quarryLimitsTestPath, token: quarryEdgeTestToken, status: http.StatusForbidden},
		{name: "generic quota read", method: http.MethodGet, path: "/api/v1/billing/orgs/org-1/quotas/quarry.crawl.page", token: quarryEdgeTestToken, status: http.StatusNoContent},
		{name: "entitlement read", method: http.MethodGet, path: "/api/v1/billing/orgs/org-1/entitlements/feature.chat", token: quarryEdgeTestToken, status: http.StatusNoContent},
		{name: "account read", method: http.MethodGet, path: "/api/v1/billing/orgs/org-1/account", token: quarryEdgeTestToken, status: http.StatusForbidden},
		{name: "usage write", method: http.MethodPost, path: "/api/v1/billing/orgs/org-1/usage", token: quarryEdgeTestToken, status: http.StatusForbidden},
		{name: "allowance write", method: http.MethodPut, path: "/api/v1/billing/orgs/org-1/allowances", token: quarryEdgeTestToken, status: http.StatusForbidden},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			request := httptest.NewRequest(tc.method, tc.path, nil)
			request.Header.Set("X-Service-Id", quarryEdgeServicePrincipal)
			if tc.token != "" {
				request.Header.Set("X-Service-Token", tc.token)
			}
			request.Header.Set(quarryLimitsOrgHeader, "org-1")
			response := httptest.NewRecorder()
			router.ServeHTTP(response, request)
			if response.Code != tc.status {
				t.Fatalf("status=%d want=%d body=%s", response.Code, tc.status, response.Body.String())
			}
		})
	}
	if len(reader.calls) != 0 {
		t.Fatalf("no refused or unrelated request may read the snapshot; calls=%v", reader.calls)
	}
}

func TestQuarryLimitsMapsStoreErrors(t *testing.T) {
	cases := []struct {
		name   string
		err    error
		status int
		code   string
	}{
		{name: "tombstoned org", err: billing.ErrOrganizationDeleted, status: http.StatusForbidden, code: "organization_inactive"},
		{name: "invalid org", err: billing.ErrAllowanceInvalid, status: http.StatusBadRequest, code: "invalid_org"},
		{name: "store down", err: errors.New("dial tcp: connection refused"), status: http.StatusServiceUnavailable, code: "limits_unavailable"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			reader := &fakeQuarryLimitsReader{err: tc.err}
			router := quarryLimitsTestServer(t, reader, time.Now().UTC())
			response := httptest.NewRecorder()
			router.ServeHTTP(response, quarryEdgeRequest(http.MethodGet, quarryLimitsTestPath, "org-1"))
			if response.Code != tc.status || decodeErrorCode(t, response.Body.Bytes()) != tc.code {
				t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
			}
			if got := response.Header().Get("Cache-Control"); got != "no-store" {
				t.Fatalf("error Cache-Control=%q", got)
			}
			if strings.Contains(response.Body.String(), "connection refused") {
				t.Fatalf("store error detail leaked: %s", response.Body.String())
			}
		})
	}

	t.Run("no reader wired", func(t *testing.T) {
		router := quarryLimitsTestServer(t, nil, time.Now().UTC())
		response := httptest.NewRecorder()
		router.ServeHTTP(response, quarryEdgeRequest(http.MethodGet, quarryLimitsTestPath, "org-1"))
		if response.Code != http.StatusServiceUnavailable {
			t.Fatalf("status=%d", response.Code)
		}
	})
}

func TestQuarryLimitsRouteMapsToTheMachineQuotaScopeOnly(t *testing.T) {
	got := serviceScopesForRequest(httptest.NewRequest(http.MethodGet, quarryLimitsTestPath, nil))
	if len(got) != 1 || got[0] != "billing:quota:read:any" {
		t.Fatalf("GET scopes=%v want [billing:quota:read:any]", got)
	}
	for _, method := range []string{http.MethodPost, http.MethodPut, http.MethodDelete, http.MethodPatch} {
		if got := serviceScopesForRequest(httptest.NewRequest(method, quarryLimitsTestPath, nil)); got != nil {
			t.Fatalf("%s scopes=%v want none", method, got)
		}
	}
	if got := serviceScopesForRequest(httptest.NewRequest(http.MethodGet, quarryLimitsTestPath+"/extra", nil)); got != nil {
		t.Fatalf("longer path scopes=%v want none", got)
	}
}

func TestQuarryEdgeRegistryPassesStartupValidationAndStaysMachineOnly(t *testing.T) {
	if err := ValidateRequiredServiceCredentialRegistry(quarryLimitsRegistry()); err != nil {
		t.Fatalf("registry with quarry-edge rejected: %v", err)
	}
	selfScope := `[{"principal":"quarry-edge","audience":"billing-core","token":"` + quarryEdgeTestToken + `","scopes":["billing:quota:read:self"]}]`
	if _, err := parseServiceCredentials(selfScope); err == nil {
		t.Fatal("quarry-edge must not be able to hold a delegated :self scope")
	}
}

func TestQuarryPlanCapsUnconfiguredIsNullAndDefaultsAreOptIn(t *testing.T) {
	off := billing.QuarryPlanCapsFor(nil, "pro", false)
	for name, value := range map[string]*int64{
		"agent_runs": off.AgentRuns, "interactive_runs": off.InteractiveRuns, "render_permits": off.RenderPermits,
		"crawl_jobs": off.CrawlJobs, "job_backlog": off.JobBacklog, "rate_multiplier": off.RateMultiplier,
	} {
		if value != nil {
			t.Fatalf("defaults off: %s=%d want null", name, *value)
		}
	}
	on := billing.QuarryPlanCapsFor(nil, "pro", true)
	if on.AgentRuns == nil || *on.AgentRuns != 8 || on.JobBacklog == nil || *on.JobBacklog != 1000 {
		t.Fatalf("pro defaults=%+v", on)
	}
	explicit := billing.QuarryPlanCapsFor(map[string]float64{billing.QuarryCapAgentRuns: 0, billing.QuarryCapCrawlJobs: -1}, "pro", false)
	if explicit.AgentRuns == nil || *explicit.AgentRuns != 0 {
		t.Fatalf("an explicit 0 is a real cap: %+v", explicit)
	}
	if explicit.CrawlJobs != nil {
		t.Fatalf("a negative explicit cap must read as null: %+v", explicit)
	}
	for _, plan := range []string{"free", "trial", "hobby", "standard", "pro", "enterprise"} {
		caps := billing.QuarryPlanCapsFor(nil, plan, true)
		if caps.AgentRuns == nil || caps.InteractiveRuns == nil || caps.RenderPermits == nil ||
			caps.CrawlJobs == nil || caps.JobBacklog == nil || caps.RateMultiplier == nil {
			t.Fatalf("plan %s is missing a placeholder cap: %+v", plan, caps)
		}
	}
}

func TestQuarryAllowanceDistinguishesUnconfiguredFromSpent(t *testing.T) {
	unconfigured := billing.QuarryAllowanceUnconfigured(billing.QuarryMetricCrawlPage)
	if unconfigured.Configured || unconfigured.Remaining != nil || unconfigured.Exhausted {
		t.Fatalf("unconfigured=%+v", unconfigured)
	}
	now := time.Date(2026, 11, 3, 10, 0, 0, 0, time.UTC)
	policy := billing.AllowancePolicy{Metric: billing.QuarryMetricCrawlPage, StartsAt: now.Add(-time.Hour)}
	spent, ok := billing.QuarryAllowanceFromReports([]billing.AllowanceReport{{
		Policy: policy, AsOf: now, Active: true,
		Current: billing.AllowancePeriod{Used: 25, Remaining: 0, To: now.Add(24 * time.Hour)},
	}}, billing.QuarryMetricCrawlPage)
	if !ok || !spent.Configured || spent.Remaining == nil || *spent.Remaining != 0 || !spent.Exhausted || *spent.Limit != 25 {
		t.Fatalf("spent=%+v ok=%v", spent, ok)
	}
	paused, ok := billing.QuarryAllowanceFromReports([]billing.AllowanceReport{{
		Policy: policy, AsOf: now, Active: false, Current: billing.AllowancePeriod{Used: 3, Remaining: 22},
	}}, billing.QuarryMetricCrawlPage)
	if !ok || !paused.Exhausted || paused.Remaining == nil || *paused.Remaining != 0 || paused.Active {
		t.Fatalf("an ended or paused policy refuses use: %+v", paused)
	}
	future := billing.AllowancePolicy{Metric: billing.QuarryMetricCrawlPage, StartsAt: now.Add(time.Hour)}
	if _, ok := billing.QuarryAllowanceFromReports([]billing.AllowanceReport{{Policy: future, AsOf: now, Active: true}}, billing.QuarryMetricCrawlPage); ok {
		t.Fatal("a policy that has not started must not answer yet")
	}
	if _, ok := billing.QuarryAllowanceFromReports([]billing.AllowanceReport{{Policy: policy, AsOf: now, Active: true}}, billing.QuarryMetricBatchURL); ok {
		t.Fatal("a policy for another metric must not answer")
	}
	quota := billing.QuarryAllowanceFromPlanQuota(billing.QuarryMetricBatchURL, 40, 15)
	if !quota.Configured || quota.Source != billing.QuarryAllowanceSourcePlanQuota || *quota.Remaining != 25 || quota.Exhausted {
		t.Fatalf("plan quota=%+v", quota)
	}
	unlimited := billing.QuarryAllowanceFromPlanQuota(billing.QuarryMetricBatchURL, -1, 15)
	if !unlimited.Unlimited || unlimited.Remaining != nil || unlimited.Exhausted {
		t.Fatalf("unlimited=%+v", unlimited)
	}
}
