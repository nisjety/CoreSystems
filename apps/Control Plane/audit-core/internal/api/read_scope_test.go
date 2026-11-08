package api

import (
	"bytes"
	"context"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"

	"github.com/triodelab/controlplane/audit-core/internal/store"
)

// WS05-S1: role-gated audit reads. These tests drive the real router with
// signed v3 delegations; the store is an in-memory stub, no database.

// scopeStub records the filters the handlers pass to the store so the tests can
// assert what was actually queried for each role.
type scopeStub struct {
	complianceStub
	auditRows      []store.AuditRow
	auditFilters   []store.AuditFilter
	usageCalls     int
	summaryCalls   int
	usageFilterOrg string
}

func (s *scopeStub) ListAudit(_ context.Context, f store.AuditFilter) ([]store.AuditRow, error) {
	s.auditFilters = append(s.auditFilters, f)
	return s.auditRows, nil
}
func (s *scopeStub) ListUsage(_ context.Context, f store.UsageFilter) ([]store.UsageRow, error) {
	s.usageCalls++
	s.usageFilterOrg = f.OrgID
	return []store.UsageRow{}, nil
}
func (s *scopeStub) SummariseUsage(context.Context, string, time.Time, time.Time) ([]store.UsageSummary, error) {
	s.summaryCalls++
	return []store.UsageSummary{}, nil
}

var scopeNonce atomic.Int64

// signScopedRead signs a v3 read delegation for an arbitrary user and role.
func signScopedRead(request *http.Request, orgID, userID, role string) {
	nonce := fmt.Sprintf("scope-nonce-%016d", scopeNonce.Add(1))
	timestamp := time.Now().UTC().Format(time.RFC3339)
	bodyDigest := sha256.Sum256(nil)
	bodySHA := base64.RawURLEncoding.EncodeToString(bodyDigest[:])
	request.Header.Set("X-Service-Id", "verevon-gateway")
	request.Header.Set("X-Service-Token", testGatewayToken)
	request.Header.Set("X-User-Id", userID)
	request.Header.Set("X-Org-Id", orgID)
	request.Header.Set("X-User-Role", role)
	request.Header.Set("X-Delegation-Version", "v3")
	request.Header.Set("X-Delegation-Timestamp", timestamp)
	request.Header.Set("X-Delegation-Nonce", nonce)
	request.Header.Set("X-Delegation-Body-SHA256", bodySHA)
	canonical := strings.Join([]string{
		"v3", "verevon-gateway", "audit-core", timestamp, nonce,
		request.Method, request.URL.RequestURI(), userID, orgID, role, bodySHA,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(testGatewayToken))
	_, _ = mac.Write([]byte(canonical))
	request.Header.Set("X-Delegation-Signature", base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
}

func scopeRouter(t *testing.T, stub *scopeStub) http.Handler {
	t.Helper()
	handler, err := New(stub, complianceTestRegistry())
	if err != nil {
		t.Fatal(err)
	}
	router := chi.NewRouter()
	handler.Mount(router)
	return router
}

func scopedGet(router http.Handler, target, userID, role string) *httptest.ResponseRecorder {
	request := httptest.NewRequest(http.MethodGet, target, nil)
	signScopedRead(request, "org-a", userID, role)
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	return recorder
}

func metaScope(t *testing.T, recorder *httptest.ResponseRecorder) string {
	t.Helper()
	meta, _ := decodeEnvelope(t, recorder)["meta"].(map[string]any)
	scope, _ := meta["scope"].(string)
	return scope
}

func setReadMode(t *testing.T, mode, environment string) {
	t.Helper()
	t.Setenv(auditReadEnforcementEnv, mode)
	t.Setenv("ENVIRONMENT", environment)
}

const auditURL = "/v1/audit?org_id=org-a"

func TestAuditReadOwnerAndAdminGetOrgRows(t *testing.T) {
	setReadMode(t, "", "")
	for _, role := range []string{"owner", "admin", "Admin"} {
		stub := &scopeStub{auditRows: []store.AuditRow{{ID: 1, UserID: "someone-else", Event: "login"}}}
		recorder := scopedGet(scopeRouter(t, stub), auditURL, "user-a", role)
		if recorder.Code != http.StatusOK {
			t.Fatalf("role %q status %d: %s", role, recorder.Code, recorder.Body.String())
		}
		if got := metaScope(t, recorder); got != scopeOrg {
			t.Fatalf("role %q meta.scope = %q, want org", role, got)
		}
		if len(stub.auditFilters) != 1 || stub.auditFilters[0].UserID != "" {
			t.Fatalf("role %q must not be narrowed to a user: %+v", role, stub.auditFilters)
		}
	}

	// An admin may also ask for any other user's rows.
	stub := &scopeStub{}
	recorder := scopedGet(scopeRouter(t, stub), auditURL+"&user_id=user-z", "user-a", "admin")
	if recorder.Code != http.StatusOK || stub.auditFilters[0].UserID != "user-z" {
		t.Fatalf("admin foreign user_id: status %d filters %+v", recorder.Code, stub.auditFilters)
	}
}

func TestAuditReadMemberViewerAndUnknownRolesGetOwnRowsOnly(t *testing.T) {
	setReadMode(t, "", "")
	// An unknown role, and a role that merely contains admin, is not an admin.
	for _, role := range []string{"member", "viewer", "guest", "superadmin", "admin,member", "unknown"} {
		stub := &scopeStub{auditRows: []store.AuditRow{{ID: 1, UserID: "user-a"}}}
		recorder := scopedGet(scopeRouter(t, stub), auditURL, "user-a", role)
		if recorder.Code != http.StatusOK {
			t.Fatalf("role %q status %d: %s", role, recorder.Code, recorder.Body.String())
		}
		if got := metaScope(t, recorder); got != scopeSelf {
			t.Fatalf("role %q meta.scope = %q, want self", role, got)
		}
		if len(stub.auditFilters) != 1 || stub.auditFilters[0].UserID != "user-a" {
			t.Fatalf("role %q must be pinned to the caller: %+v", role, stub.auditFilters)
		}
	}

	// Asking for their own user_id explicitly is fine and stays self scope.
	stub := &scopeStub{}
	recorder := scopedGet(scopeRouter(t, stub), auditURL+"&user_id=user-a", "user-a", "member")
	if recorder.Code != http.StatusOK || metaScope(t, recorder) != scopeSelf {
		t.Fatalf("own user_id: status %d %s", recorder.Code, recorder.Body.String())
	}
}

func TestAuditReadForeignUserIDIsATypedForbidden(t *testing.T) {
	setReadMode(t, "", "")
	for _, role := range []string{"member", "viewer", ""} {
		stub := &scopeStub{}
		router := scopeRouter(t, stub)
		request := httptest.NewRequest(http.MethodGet, auditURL+"&user_id=user-b", nil)
		signScopedRead(request, "org-a", "user-a", role)
		recorder := httptest.NewRecorder()
		router.ServeHTTP(recorder, request)
		if role == "" {
			// A delegation with an empty role never verifies at all.
			if recorder.Code != http.StatusForbidden || len(stub.auditFilters) != 0 {
				t.Fatalf("empty role status %d, store calls %d", recorder.Code, len(stub.auditFilters))
			}
			continue
		}
		if recorder.Code != http.StatusForbidden || envelopeErrorCode(t, recorder) != "audit_scope_denied" {
			t.Fatalf("role %q status %d code %q: %s", role, recorder.Code, envelopeErrorCode(t, recorder), recorder.Body.String())
		}
		failure, _ := decodeEnvelope(t, recorder)["error"].(map[string]any)
		details, _ := failure["details"].(map[string]any)
		if failure["message"] == "" || details["scope"] != scopeSelf {
			t.Fatalf("typed envelope needs message and details.scope=self: %v", failure)
		}
		if len(stub.auditFilters) != 0 {
			t.Fatalf("a refused request must not reach the store: %+v", stub.auditFilters)
		}
	}
}

// The delegation layer already refuses an empty user id; the handler keeps its
// own guard so a future change there cannot open a member read to the org.
func TestAuditReadEmptyDelegatedUserIDIsForbidden(t *testing.T) {
	setReadMode(t, "", "")
	stub := &scopeStub{}
	router := scopeRouter(t, stub)
	request := httptest.NewRequest(http.MethodGet, auditURL, nil)
	signScopedRead(request, "org-a", "", "member")
	recorder := httptest.NewRecorder()
	router.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusForbidden || len(stub.auditFilters) != 0 {
		t.Fatalf("empty user id: status %d store calls %d", recorder.Code, len(stub.auditFilters))
	}

	// Handler-level guard, with the authorization injected directly.
	handler, err := New(stub, complianceTestRegistry())
	if err != nil {
		t.Fatal(err)
	}
	direct := httptest.NewRequest(http.MethodGet, auditURL, nil)
	direct = direct.WithContext(context.WithValue(direct.Context(), serviceAuthorizationContextKey{},
		serviceAuthorization{OrgID: "org-a", UserID: "  ", UserRole: "member"}))
	rec := httptest.NewRecorder()
	handler.listAudit(rec, direct)
	if rec.Code != http.StatusForbidden || envelopeErrorCode(t, rec) != "audit_scope_denied" || len(stub.auditFilters) != 0 {
		t.Fatalf("empty caller: status %d code %q", rec.Code, envelopeErrorCode(t, rec))
	}
}

func TestAuditReadToolActionWithoutUserIDIsOrgRedacted(t *testing.T) {
	setReadMode(t, "", "")
	rows := []store.AuditRow{
		{
			ID: 1, OrgID: "org-a", UserID: "user-b", ActorRole: "admin", Plane: "model", Event: "tool_action",
			Subject: "secret subject", ResourceID: "res-1", Outcome: "ok", RequestID: "req-1", EventID: "evt-1",
			Details: map[string]any{
				"tool": "web_search", "data_category": "public", "zdr": true, "phase": "complete",
				"query": "secret query", "nested": map[string]any{"a": 1},
			},
		},
		{ID: 2, OrgID: "org-a", UserID: "user-b", Plane: "control", Event: "login", Outcome: "ok", Details: map[string]any{"ip": "1.2.3.4"}},
		{
			ID: 3, OrgID: "org-a", Plane: "model", Event: "tool_action", Outcome: "denied",
			// Whitelisted keys that carry a non-scalar value are dropped.
			Details: map[string]any{"tool": map[string]any{"deep": "x"}, "data_category": []any{"a"}, "zdr": false},
		},
	}
	stub := &scopeStub{auditRows: rows}
	recorder := scopedGet(scopeRouter(t, stub), auditURL+"&event=tool_action", "user-a", "member")
	if recorder.Code != http.StatusOK {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if got := metaScope(t, recorder); got != scopeOrgRedacted {
		t.Fatalf("meta.scope = %q, want org_redacted", got)
	}
	if stub.auditFilters[0].UserID != "" || stub.auditFilters[0].Event != "tool_action" {
		t.Fatalf("org-wide tool_action must not be pinned to the caller: %+v", stub.auditFilters[0])
	}

	envelope := decodeEnvelope(t, recorder)
	data, _ := envelope["data"].([]any)
	if len(data) != 2 {
		t.Fatalf("non tool_action rows must be dropped, got %d rows: %s", len(data), recorder.Body.String())
	}
	meta, _ := envelope["meta"].(map[string]any)
	if meta["count"] != float64(2) {
		t.Fatalf("meta.count = %v, want the redacted count 2", meta["count"])
	}
	first, _ := data[0].(map[string]any)
	allowedTop := map[string]bool{"id": true, "occurred_at": true, "ingested_at": true, "plane": true, "event": true, "outcome": true, "details": true}
	for key := range first {
		if !allowedTop[key] {
			t.Fatalf("redacted row leaks key %q: %v", key, first)
		}
	}
	details, _ := first["details"].(map[string]any)
	if len(details) != 4 || details["tool"] != "web_search" || details["data_category"] != "public" ||
		details["zdr"] != true || details["phase"] != "complete" {
		t.Fatalf("details = %v, want only the four whitelisted scalars", details)
	}
	second, _ := data[1].(map[string]any)
	secondDetails, _ := second["details"].(map[string]any)
	if len(secondDetails) != 1 || secondDetails["zdr"] != false {
		t.Fatalf("non-scalar whitelisted values must be dropped: %v", secondDetails)
	}
	body := recorder.Body.String()
	for _, secret := range []string{"user-b", "secret subject", "res-1", "secret query", "1.2.3.4", "req-1", "evt-1", "actor_role"} {
		if strings.Contains(body, secret) {
			t.Fatalf("redacted response leaks %q: %s", secret, body)
		}
	}
}

func TestAuditReadToolActionWithOwnUserIDStaysSelfScope(t *testing.T) {
	setReadMode(t, "", "")
	stub := &scopeStub{auditRows: []store.AuditRow{{ID: 1, UserID: "user-a", Event: "tool_action", Subject: "mine"}}}
	recorder := scopedGet(scopeRouter(t, stub), auditURL+"&event=tool_action&user_id=user-a", "user-a", "member")
	if recorder.Code != http.StatusOK || metaScope(t, recorder) != scopeSelf {
		t.Fatalf("status %d: %s", recorder.Code, recorder.Body.String())
	}
	if !strings.Contains(recorder.Body.String(), "mine") {
		t.Fatalf("self scope returns the caller's full rows: %s", recorder.Body.String())
	}
}

func TestUsageRoutesAreAdminOnly(t *testing.T) {
	setReadMode(t, "", "")
	for _, target := range []string{"/v1/usage?org_id=org-a", "/v1/usage/summary?org_id=org-a"} {
		for _, role := range []string{"member", "viewer", "unknown"} {
			stub := &scopeStub{}
			recorder := scopedGet(scopeRouter(t, stub), target, "user-a", role)
			if recorder.Code != http.StatusForbidden || envelopeErrorCode(t, recorder) != "audit_scope_denied" {
				t.Fatalf("%s as %q: status %d %s", target, role, recorder.Code, recorder.Body.String())
			}
			if stub.usageCalls != 0 || stub.summaryCalls != 0 {
				t.Fatalf("%s as %q reached the store", target, role)
			}
		}
		for _, role := range []string{"owner", "admin"} {
			stub := &scopeStub{}
			recorder := scopedGet(scopeRouter(t, stub), target, "user-a", role)
			if recorder.Code != http.StatusOK {
				t.Fatalf("%s as %q: status %d %s", target, role, recorder.Code, recorder.Body.String())
			}
		}
	}
}

func TestAuditReadModeProductionRefusesNonEnforceMode(t *testing.T) {
	for _, mode := range []string{"report", "off", "REPORT", " off "} {
		if got, err := auditReadMode(mode, "production"); err == nil {
			t.Fatalf("mode %q accepted in production as %q", mode, got)
		}
		if got, err := auditReadMode(mode, " Production "); err == nil {
			t.Fatalf("mode %q accepted in production (case/space) as %q", mode, got)
		}
		if got, err := auditReadMode(mode, "staging"); err != nil || (got != auditReadModeReport && got != auditReadModeOff) {
			t.Fatalf("mode %q outside production: got %q err %v", mode, got, err)
		}
	}
	if _, err := auditReadMode("lenient", "development"); err == nil {
		t.Fatal("an unknown mode must be an error")
	}
	if _, err := auditReadMode("lenient", "production"); err == nil {
		t.Fatal("an unknown mode must be an error in production too")
	}
	for _, raw := range []string{"", "  ", "enforce", "ENFORCE"} {
		for _, environment := range []string{"", "production", "development"} {
			if got, err := auditReadMode(raw, environment); err != nil || got != auditReadModeEnforce {
				t.Fatalf("auditReadMode(%q,%q) = %q, %v; want enforce", raw, environment, got, err)
			}
		}
	}
}

func TestNewRefusesToStartNonEnforcingInProduction(t *testing.T) {
	setReadMode(t, "report", "production")
	if _, err := New(&scopeStub{}, complianceTestRegistry()); err == nil {
		t.Fatal("New must refuse report mode when ENVIRONMENT=production")
	}
	setReadMode(t, "off", "production")
	if _, err := New(&scopeStub{}, complianceTestRegistry()); err == nil {
		t.Fatal("New must refuse off mode when ENVIRONMENT=production")
	}
	setReadMode(t, "nonsense", "development")
	if _, err := New(&scopeStub{}, complianceTestRegistry()); err == nil {
		t.Fatal("New must refuse an unknown mode")
	}
	setReadMode(t, "", "production")
	handler, err := New(&scopeStub{}, complianceTestRegistry())
	if err != nil || handler.readMode != auditReadModeEnforce {
		t.Fatalf("unset mode in production must enforce: %v mode=%q", err, handler.readMode)
	}
}

func TestAuditReadReportModeServesOrgRowsAndLogsWouldDeny(t *testing.T) {
	setReadMode(t, "report", "development")
	var logs bytes.Buffer
	previous := log.Logger
	log.Logger = zerolog.New(&logs)
	t.Cleanup(func() { log.Logger = previous })

	stub := &scopeStub{auditRows: []store.AuditRow{{ID: 1, UserID: "user-b"}}}
	router := scopeRouter(t, stub)
	recorder := scopedGet(router, auditURL+"&user_id=user-b", "user-a", "member")
	if recorder.Code != http.StatusOK || metaScope(t, recorder) != scopeOrg {
		t.Fatalf("report mode must serve org rows: status %d %s", recorder.Code, recorder.Body.String())
	}
	if stub.auditFilters[0].UserID != "user-b" {
		t.Fatalf("report mode must not narrow the filter: %+v", stub.auditFilters[0])
	}
	if !strings.Contains(logs.String(), "would be narrowed or denied") || !strings.Contains(logs.String(), `"route":"/v1/audit"`) {
		t.Fatalf("report mode must log the would-deny decision: %s", logs.String())
	}

	logs.Reset()
	usage := scopedGet(router, "/v1/usage?org_id=org-a", "user-a", "member")
	if usage.Code != http.StatusOK || stub.usageCalls != 1 {
		t.Fatalf("report mode usage: status %d calls %d", usage.Code, stub.usageCalls)
	}
	if !strings.Contains(logs.String(), "would be narrowed or denied") {
		t.Fatalf("usage would-deny not logged: %s", logs.String())
	}

	// An admin in report mode logs nothing.
	logs.Reset()
	scopedGet(router, auditURL, "user-a", "admin")
	if strings.Contains(logs.String(), "would be narrowed or denied") {
		t.Fatalf("an admin read must not log would-deny: %s", logs.String())
	}
}

func TestAuditReadOffModeServesAsBefore(t *testing.T) {
	setReadMode(t, "off", "development")
	stub := &scopeStub{}
	router := scopeRouter(t, stub)
	recorder := scopedGet(router, auditURL+"&user_id=user-b", "user-a", "viewer")
	if recorder.Code != http.StatusOK || metaScope(t, recorder) != scopeOrg {
		t.Fatalf("off mode: status %d %s", recorder.Code, recorder.Body.String())
	}
	if stub.auditFilters[0].UserID != "user-b" {
		t.Fatalf("off mode must not narrow: %+v", stub.auditFilters[0])
	}
	for _, target := range []string{"/v1/usage?org_id=org-a", "/v1/usage/summary?org_id=org-a"} {
		if got := scopedGet(router, target, "user-a", "viewer"); got.Code != http.StatusOK {
			t.Fatalf("off mode %s: status %d", target, got.Code)
		}
	}
}

func TestRedactToolActionRowsIsAWhitelist(t *testing.T) {
	now := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)
	out := redactToolActionRows([]store.AuditRow{
		{ID: 9, OccurredAt: now, IngestedAt: now, Plane: "model", Event: "tool_action", Outcome: "ok", Details: nil},
		{ID: 10, Event: "member_added"},
	})
	if len(out) != 1 || out[0].ID != 9 || out[0].Details == nil || len(out[0].Details) != 0 {
		t.Fatalf("redaction = %+v", out)
	}
	raw, err := json.Marshal(out[0])
	if err != nil {
		t.Fatal(err)
	}
	var keys map[string]any
	_ = json.Unmarshal(raw, &keys)
	if len(keys) != 7 {
		t.Fatalf("redacted row must have exactly 7 keys, got %v", keys)
	}
}
