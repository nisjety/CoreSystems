package handler

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	chimw "github.com/go-chi/chi/v5/middleware"

	"github.com/triodelab/dataplane/services/data-quality-go/internal/authctx"
	"github.com/triodelab/dataplane/services/data-quality-go/internal/policy"
)

// These tests run the REAL authentication middleware, the REAL scope check, the REAL
// route table (MountProtectedRoutes) and the REAL handler. The only stand-in is the
// store behind the handler: what it must do with a row (row-level security, the audit
// row in the same transaction, the delete on reset) is proved against Postgres in
// internal/policy (store_integration_test.go). What is proved here is who may call,
// for which organization, with which input, and what the handler asks the store for.

const adminScope = "data:quality:admin"

// tokenVerifier turns a fixed set of test tokens into verified claims, the way
// authctx.Verifier would for a signed token (the identity-shape rules themselves are
// tested in authctx).
type tokenVerifier map[string]*authctx.Claims

func (v tokenVerifier) Verify(token string) (*authctx.Claims, error) {
	claims, ok := v[token]
	if !ok {
		return nil, errors.New("invalid token")
	}
	copied := *claims
	copied.Verified = true
	return &copied, nil
}

func serviceClaims(serviceID, org string, scopes ...string) *authctx.Claims {
	claims := &authctx.Claims{OrgID: org, ServiceID: serviceID, PrincipalType: "service", Scopes: scopes}
	claims.Subject = serviceID
	return claims
}

func userClaims(userID, org string, scopes ...string) *authctx.Claims {
	claims := &authctx.Claims{OrgID: org, UserID: userID, PrincipalType: "user", Scopes: scopes}
	claims.Subject = userID
	return claims
}

var testTokens = tokenVerifier{
	"operator-a":      serviceClaims("corpus-seeder", "org-a", adminScope),
	"operator-b":      serviceClaims("corpus-seeder", "org-b", adminScope),
	"operator-noscop": serviceClaims("corpus-seeder", "org-a", "data:read"),
	"admin-user-a":    userClaims("user-1", "org-a", adminScope, "data:read"),
	"member-user-a":   userClaims("user-2", "org-a", "data:read"),
}

// fakeStore keeps policies in memory with the semantics the handler relies on:
// Put replaces the whole policy, and an input for the default is a reset.
type fakeStore struct {
	mu       sync.Mutex
	policies map[string]policy.Record
	puts     []putCall
	gets     []string
	err      error
}

type putCall struct {
	Org   string
	Input policy.Input
	Actor policy.Actor
}

func newFakeStore() *fakeStore { return &fakeStore{policies: map[string]policy.Record{}} }

func (s *fakeStore) Get(_ context.Context, orgID string) (policy.Record, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.gets = append(s.gets, orgID)
	if s.err != nil {
		return policy.Record{}, s.err
	}
	if record, ok := s.policies[orgID]; ok {
		return record, nil
	}
	return policy.Default(orgID), nil
}

func (s *fakeStore) Put(_ context.Context, orgID string, in policy.Input, actor policy.Actor) (policy.Result, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.puts = append(s.puts, putCall{Org: orgID, Input: in, Actor: actor})
	if s.err != nil {
		return policy.Result{}, s.err
	}
	before, ok := s.policies[orgID]
	if !ok {
		before = policy.Default(orgID)
	}
	if in.IsDefault() {
		delete(s.policies, orgID)
		return policy.Result{Action: "reset", Before: before, After: policy.Default(orgID)}, nil
	}
	after := policy.Record{Exists: true, Policy: policy.Policy{
		OrgID:                orgID,
		ContextualRetrieval:  in.ContextualRetrieval,
		GraphExtraction:      in.GraphExtraction,
		ContextPromptVersion: in.ContextPromptVersion,
		BudgetMode:           in.BudgetMode,
		MonthlyBudgetUSD:     in.MonthlyBudgetUSD,
		Reason:               in.Reason,
		UpdatedBy:            actor.ID,
		UpdatedAt:            time.Date(2026, 10, 7, 9, 0, 0, 0, time.UTC),
	}}
	s.policies[orgID] = after
	return policy.Result{Action: "set", Before: before, After: after}, nil
}

func (s *fakeStore) putCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.puts)
}

// routerWith mounts the real protected route table over the test tokens.
func routerWith(store PolicyStore) http.Handler {
	router := chi.NewRouter()
	// The service mounts this at its root (cmd/main.go), before the protected routes.
	router.Use(chimw.RequestID)
	h := &QualityHandler{}
	if store != nil {
		h.WithPolicyStore(store)
	}
	MountProtectedRoutes(router, authctx.Middleware(testTokens), h)
	return router
}

type reply struct {
	Code int
	Body map[string]any
	Raw  string
}

func call(t *testing.T, router http.Handler, method, path, token, body string, headers ...string) reply {
	t.Helper()
	var reader *bytes.Reader
	if body != "" {
		reader = bytes.NewReader([]byte(body))
	} else {
		reader = bytes.NewReader(nil)
	}
	req := httptest.NewRequest(method, path, reader)
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	for i := 0; i+1 < len(headers); i += 2 {
		req.Header.Set(headers[i], headers[i+1])
	}
	rec := httptest.NewRecorder()
	router.ServeHTTP(rec, req)
	out := reply{Code: rec.Code, Raw: rec.Body.String()}
	_ = json.Unmarshal(rec.Body.Bytes(), &out.Body)
	return out
}

func policyPath(org string) string { return "/v1/admin/orgs/" + org + "/processing-policy" }

const offBody = `{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit","reason":"eval baseline organization"}`
const inheritBody = `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit"}`

func errorCode(r reply) string {
	if code, ok := r.Body["code"].(string); ok {
		return code
	}
	// authctx's own errors nest the code.
	if nested, ok := r.Body["error"].(map[string]any); ok {
		if code, ok := nested["code"].(string); ok {
			return code
		}
	}
	return ""
}

// ---- authorization: who may call --------------------------------------------------

func TestPolicyRoutesRequireAuthentication(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)
	for _, method := range []string{http.MethodGet, http.MethodPut} {
		got := call(t, router, method, policyPath("org-a"), "", offBody)
		if got.Code != http.StatusUnauthorized {
			t.Fatalf("%s without a token = %d, want 401", method, got.Code)
		}
		got = call(t, router, method, policyPath("org-a"), "not-a-token", offBody)
		if got.Code != http.StatusUnauthorized {
			t.Fatalf("%s with an invalid token = %d, want 401", method, got.Code)
		}
	}
	if store.putCount() != 0 || len(store.gets) != 0 {
		t.Fatal("an unauthenticated request reached the store")
	}
}

func TestPolicyRoutesRequireTheAdminScope(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)
	for _, token := range []string{"operator-noscop", "member-user-a"} {
		for _, method := range []string{http.MethodGet, http.MethodPut} {
			got := call(t, router, method, policyPath("org-a"), token, offBody)
			if got.Code != http.StatusForbidden {
				t.Fatalf("%s %s = %d, want 403 (the scope %s is required)", method, token, got.Code, adminScope)
			}
		}
	}
	if store.putCount() != 0 || len(store.gets) != 0 {
		t.Fatal("a request without the admin scope reached the store")
	}
}

// The owner ruling: per-org off is operator-only, no org-admin UI. An organization's
// own owner or admin carries data:quality:admin in their interactive token, so the scope
// alone would let them switch background processing off for their organization.
func TestAnOrganizationAdminCannotChangeThePolicyButMayReadIt(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)

	got := call(t, router, http.MethodPut, policyPath("org-a"), "admin-user-a", offBody)
	if got.Code != http.StatusForbidden || errorCode(got) != "operator_only" {
		t.Fatalf("PUT as an org admin = %d %s, want 403 operator_only", got.Code, got.Raw)
	}
	if store.putCount() != 0 {
		t.Fatal("an organization admin's PUT reached the store")
	}

	got = call(t, router, http.MethodGet, policyPath("org-a"), "admin-user-a", "")
	if got.Code != http.StatusOK {
		t.Fatalf("GET as an org admin = %d %s, want 200: reading your own organization's policy is transparent", got.Code, got.Raw)
	}
}

// Cross-org write refused: the token's organization is the authority, never the path.
func TestAnOperatorTokenOfOneOrganizationCannotWriteOrReadAnother(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)

	for _, method := range []string{http.MethodPut, http.MethodGet} {
		got := call(t, router, method, policyPath("org-b"), "operator-a", offBody)
		if got.Code != http.StatusForbidden || errorCode(got) != "tenant_mismatch" {
			t.Fatalf("%s org-b with an org-a token = %d %s, want 403 tenant_mismatch", method, got.Code, got.Raw)
		}
	}
	if store.putCount() != 0 || len(store.gets) != 0 {
		t.Fatalf("a cross-organization request reached the store: puts=%d gets=%v", store.putCount(), store.gets)
	}

	// And the same operator can do both for its own organization.
	if got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", offBody); got.Code != http.StatusOK {
		t.Fatalf("PUT own org = %d %s", got.Code, got.Raw)
	}
	if store.puts[0].Org != "org-a" {
		t.Fatalf("the store was asked to write %q, want the token's org-a", store.puts[0].Org)
	}
}

func TestAForgedTenantHeaderIsRefusedByTheAuthBoundary(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)
	got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", offBody, "X-Org-ID", "org-b")
	if got.Code != http.StatusForbidden {
		t.Fatalf("a conflicting X-Org-ID = %d, want 403", got.Code)
	}
	if store.putCount() != 0 {
		t.Fatal("the request reached the store")
	}
}

func TestThePathOrganizationIsValidated(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)
	for name, org := range map[string]string{
		"a space":                 "org%20a",
		"an encoded slash":        "org%2Fa",
		"a dot dot":               "..",
		"a leading dash":          "-org",
		"a control character":     "org%0Aa",
		"a sql quote":             "org%27--",
		"longer than 128 letters": strings.Repeat("a", 129),
	} {
		t.Run(name, func(t *testing.T) {
			got := call(t, router, http.MethodPut, policyPath(org), "operator-a", offBody)
			if got.Code != http.StatusBadRequest || errorCode(got) != "invalid_org" {
				t.Fatalf("PUT %q = %d %s, want 400 invalid_org", org, got.Code, got.Raw)
			}
		})
	}
	if store.putCount() != 0 {
		t.Fatal("an invalid organization id reached the store")
	}
}

// The handler does not rely on being mounted behind the route's middleware: mounted
// bare, behind authentication only, it still refuses a user and a foreign organization.
func TestThePutHandlerRefusesOnItsOwnWhenMountedWithoutTheRouteMiddleware(t *testing.T) {
	store := newFakeStore()
	h := (&QualityHandler{}).WithPolicyStore(store)
	router := chi.NewRouter()
	router.With(authctx.Middleware(testTokens)).Put("/v1/admin/orgs/{org}/processing-policy", h.PutPolicy)
	router.With(authctx.Middleware(testTokens)).Get("/v1/admin/orgs/{org}/processing-policy", h.GetPolicy)

	if got := call(t, router, http.MethodPut, policyPath("org-a"), "admin-user-a", offBody); got.Code != http.StatusForbidden {
		t.Fatalf("a user principal = %d, want 403", got.Code)
	}
	if got := call(t, router, http.MethodPut, policyPath("org-b"), "operator-a", offBody); got.Code != http.StatusForbidden {
		t.Fatalf("a foreign organization = %d, want 403", got.Code)
	}
	if got := call(t, router, http.MethodGet, policyPath("org-b"), "operator-a", ""); got.Code != http.StatusForbidden {
		t.Fatalf("GET a foreign organization = %d, want 403", got.Code)
	}
	if store.putCount() != 0 || len(store.gets) != 0 {
		t.Fatal("a refused request reached the store")
	}
}

// ---- the write -------------------------------------------------------------------

func TestAnOperatorSetsAPolicyAndTheChangeIsAttributed(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)

	got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", `{
		"contextual_retrieval": " off ",
		"graph_extraction": "inherit",
		"context_prompt_version": "ctx-v2",
		"budget_mode": "inherit",
		"reason": "  eval baseline organization  "
	}`, "X-Request-Id", "req-123")
	if got.Code != http.StatusOK {
		t.Fatalf("PUT = %d %s", got.Code, got.Raw)
	}

	if len(store.puts) != 1 {
		t.Fatalf("store puts = %d, want 1", len(store.puts))
	}
	call0 := store.puts[0]
	if call0.Org != "org-a" || call0.Actor.ID != "corpus-seeder" || call0.Actor.Type != "service" {
		t.Fatalf("store call = %+v, want org-a by the verified service principal", call0)
	}
	in := call0.Input
	if in.ContextualRetrieval != "off" || in.Reason == nil || *in.Reason != "eval baseline organization" {
		t.Fatalf("the input was not normalized: %+v", in)
	}
	if in.ContextPromptVersion == nil || *in.ContextPromptVersion != "ctx-v2" {
		t.Fatalf("the pin was lost: %+v", in)
	}

	if got.Body["org_id"] != "org-a" || got.Body["action"] != "set" {
		t.Fatalf("response = %s", got.Raw)
	}
	effective, _ := got.Body["effective"].(map[string]any)
	if effective["contextual_retrieval"] != "off" {
		t.Fatalf("effective = %v, want contextual_retrieval off", effective)
	}
	previous, _ := got.Body["previous"].(map[string]any)
	if previous["source"] != "default" {
		t.Fatalf("previous = %v, want the default it replaced", previous)
	}
	view, _ := got.Body["policy"].(map[string]any)
	if view["source"] != "policy" || view["updated_by"] != "corpus-seeder" {
		t.Fatalf("policy view = %v", view)
	}
}

// off needs a reason (an audited exception, never silent), and so does the one
// budget mode that withholds work.
func TestAnOffOrAStopWithoutAReasonIsRefused(t *testing.T) {
	enforceEverything(t) // so that the refusal is for the missing reason, not for what nothing enforces yet
	for name, body := range map[string]string{
		"contextual off":     `{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit"}`,
		"graph off":          `{"contextual_retrieval":"inherit","graph_extraction":"off","budget_mode":"inherit"}`,
		"blank reason":       `{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit","reason":"   "}`,
		"stop":               `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"stop"}`,
		"reason is a number": `{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit","reason":7}`,
	} {
		t.Run(name, func(t *testing.T) {
			store := newFakeStore()
			got := call(t, routerWith(store), http.MethodPut, policyPath("org-a"), "operator-a", body)
			if got.Code != http.StatusBadRequest {
				t.Fatalf("PUT = %d %s, want 400", got.Code, got.Raw)
			}
			if store.putCount() != 0 {
				t.Fatal("an invalid policy reached the store")
			}
		})
	}
}

func TestAPutMustStateEverySwitchAndNothingElse(t *testing.T) {
	enforceEverything(t) // the budget cases test the value rules, not the enforcement gate
	for name, body := range map[string]string{
		"no contextual_retrieval": `{"graph_extraction":"inherit","budget_mode":"inherit"}`,
		"no graph_extraction":     `{"contextual_retrieval":"inherit","budget_mode":"inherit"}`,
		"no budget_mode":          `{"contextual_retrieval":"inherit","graph_extraction":"inherit"}`,
		"an empty object":         `{}`,
		"null switch":             `{"contextual_retrieval":null,"graph_extraction":"inherit","budget_mode":"inherit"}`,
		"an unknown field":        `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit","org_id":"org-b"}`,
		"a bad switch":            `{"contextual_retrieval":"disabled","graph_extraction":"inherit","budget_mode":"inherit"}`,
		"a bad budget mode":       `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"unlimited"}`,
		"a bad prompt version":    `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit","context_prompt_version":"ctx-v3"}`,
		"a blank prompt version":  `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit","context_prompt_version":""}`,
		"a negative budget":       `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"alert_only","monthly_budget_usd":-1}`,
		"an absurd budget":        `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"alert_only","monthly_budget_usd":1e12}`,
		"not json":                `contextual_retrieval=off`,
		"two objects":             inheritBody + inheritBody,
		"trailing garbage":        inheritBody + ` x`,
		"an array":                `[]`,
		"a reason with a newline": `{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit","reason":"a\nb"}`,
		"an overlong reason":      `{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit","reason":"` + strings.Repeat("x", 501) + `"}`,
		"an oversized body":       `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit","reason":"` + strings.Repeat("x", 9000) + `"}`,
	} {
		t.Run(name, func(t *testing.T) {
			store := newFakeStore()
			got := call(t, routerWith(store), http.MethodPut, policyPath("org-a"), "operator-a", body)
			if got.Code != http.StatusBadRequest || errorCode(got) != "invalid_request" {
				t.Fatalf("PUT = %d %s, want 400 invalid_request", got.Code, got.Raw)
			}
			if store.putCount() != 0 {
				t.Fatal("an invalid policy reached the store")
			}
		})
	}
}

// The API never claims an effect it does not have: the budget is not read by the budget gate yet
// (CR-T9b), so a value for it is refused, not stored as a silent no-op that an operator would take for
// a real stop. Graph extraction is read by graph-index (CR-T12) and is accepted: see
// TestGraphExtractionIsAcceptedBecauseGraphIndexReadsIt.
func TestWhatNothingEnforcesYetIsRefusedByTheEndpoint(t *testing.T) {
	for name, body := range map[string]string{
		"stop":       `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"stop","reason":"cap"}`,
		"alert only": `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"alert_only"}`,
		"a budget":   `{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit","monthly_budget_usd":10}`,
	} {
		t.Run(name, func(t *testing.T) {
			store := newFakeStore()
			got := call(t, routerWith(store), http.MethodPut, policyPath("org-a"), "operator-a", body)
			if got.Code != http.StatusBadRequest || !strings.Contains(got.Raw, "silent no-op") {
				t.Fatalf("PUT = %d %s, want 400 saying a value nothing honours would be a silent no-op", got.Code, got.Raw)
			}
			if store.putCount() != 0 {
				t.Fatal("a value nothing enforces was stored")
			}
		})
	}
}

// graph-index (CR-T12) reads graph_extraction before every extraction, so the endpoint accepts it with
// the default enforcement (the test does not call enforceEverything): an organization set to off ends in
// the state skipped_policy there, with no model call, and the response says what the value means.
func TestGraphExtractionIsAcceptedBecauseGraphIndexReadsIt(t *testing.T) {
	for name, tc := range map[string]struct{ body, want string }{
		"off with a reason": {`{"contextual_retrieval":"inherit","graph_extraction":"off","budget_mode":"inherit","reason":"incident INC-7"}`, "off"},
		"on":                {`{"contextual_retrieval":"inherit","graph_extraction":"on","budget_mode":"inherit"}`, "on"},
	} {
		t.Run(name, func(t *testing.T) {
			store := newFakeStore()
			got := call(t, routerWith(store), http.MethodPut, policyPath("org-a"), "operator-a", tc.body)
			if got.Code != http.StatusOK {
				t.Fatalf("PUT = %d %s, want 200: graph-index reads graph_extraction", got.Code, got.Raw)
			}
			effective, _ := got.Body["effective"].(map[string]any)
			if effective["graph_extraction"] != tc.want {
				t.Fatalf("effective graph_extraction = %v, want %s (%s)", effective["graph_extraction"], tc.want, got.Raw)
			}
			if store.putCount() != 1 {
				t.Fatalf("the store was written %d times, want once", store.putCount())
			}
		})
	}
	// An off is an audited exception: without a reason it is still refused.
	store := newFakeStore()
	got := call(t, routerWith(store), http.MethodPut, policyPath("org-a"), "operator-a",
		`{"contextual_retrieval":"inherit","graph_extraction":"off","budget_mode":"inherit"}`)
	if got.Code != http.StatusBadRequest || store.putCount() != 0 {
		t.Fatalf("PUT graph off without a reason = %d (puts %d), want 400 and nothing stored", got.Code, store.putCount())
	}
}

// All switches at their defaults is a reset: the row goes away, so a row exists only
// because an operator set something.
func TestPuttingEverySwitchToInheritResetsThePolicy(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)
	if got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", offBody); got.Code != http.StatusOK {
		t.Fatalf("PUT off = %d %s", got.Code, got.Raw)
	}
	got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", inheritBody)
	if got.Code != http.StatusOK || got.Body["action"] != "reset" {
		t.Fatalf("PUT inherit = %d %s, want a reset", got.Code, got.Raw)
	}
	previous, _ := got.Body["previous"].(map[string]any)
	if previous["contextual_retrieval"] != "off" {
		t.Fatalf("previous = %v, want the off policy that was lifted", previous)
	}
	read := call(t, router, http.MethodGet, policyPath("org-a"), "operator-a", "")
	view, _ := read.Body["policy"].(map[string]any)
	effective, _ := read.Body["effective"].(map[string]any)
	if view["source"] != "default" || effective["contextual_retrieval"] != "on" {
		t.Fatalf("GET after the reset = %s, want the default (on)", read.Raw)
	}
}

// ---- the read --------------------------------------------------------------------

func TestAnAbsentPolicyReadsAsTheDefaultWhichIsOn(t *testing.T) {
	got := call(t, routerWith(newFakeStore()), http.MethodGet, policyPath("org-a"), "operator-a", "")
	if got.Code != http.StatusOK {
		t.Fatalf("GET = %d %s", got.Code, got.Raw)
	}
	view, _ := got.Body["policy"].(map[string]any)
	effective, _ := got.Body["effective"].(map[string]any)
	if view["source"] != "default" || view["contextual_retrieval"] != "inherit" || effective["contextual_retrieval"] != "on" {
		t.Fatalf("GET of an org without a row = %s, want inherit/on from the default", got.Raw)
	}
	if view["updated_by"] != nil || view["updated_at"] != nil {
		t.Fatalf("a default invents no author or time: %v", view)
	}
}

// An organization's own admin may read how its data is processed, but not an operator's internal note
// about why, nor which operator wrote it. An operator sees both.
func TestAnOrganizationAdminDoesNotSeeTheOperatorsReasonOrIdentity(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)
	if got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", offBody); got.Code != http.StatusOK {
		t.Fatalf("PUT = %d %s", got.Code, got.Raw)
	}

	asAdmin := call(t, router, http.MethodGet, policyPath("org-a"), "admin-user-a", "")
	view, _ := asAdmin.Body["policy"].(map[string]any)
	if view["contextual_retrieval"] != "off" || view["source"] != "policy" {
		t.Fatalf("the admin cannot see the state of the policy: %v", view)
	}
	if view["reason"] != nil || view["updated_by"] != nil || strings.Contains(asAdmin.Raw, "eval baseline") || strings.Contains(asAdmin.Raw, "corpus-seeder") {
		t.Fatalf("an operator's internal note or identity reached an organization admin: %s", asAdmin.Raw)
	}

	asOperator := call(t, router, http.MethodGet, policyPath("org-a"), "operator-a", "")
	operatorView, _ := asOperator.Body["policy"].(map[string]any)
	if operatorView["reason"] != "eval baseline organization" || operatorView["updated_by"] != "corpus-seeder" {
		t.Fatalf("the operator does not see its own note: %v", operatorView)
	}
}

// The request id in the audit trail is client-supplied (X-Request-Id): bounded and printable.
func TestTheAuditedRequestIDIsBoundedAndPrintable(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)
	long := strings.Repeat("a", 500)
	if got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", offBody, "X-Request-Id", long+"\x7f\x01 \t"); got.Code != http.StatusOK {
		t.Fatalf("PUT = %d %s", got.Code, got.Raw)
	}
	if id := store.puts[0].Actor.RequestID; id != strings.Repeat("a", maxRequestIDLength) {
		t.Fatalf("request id = %q (%d chars), want %d printable characters", id, len(id), maxRequestIDLength)
	}
	if got := sanitizeRequestID("req-1\n\x00x y"); got != "req-1xy" {
		t.Fatalf("sanitizeRequestID = %q, want control characters and spaces dropped", got)
	}
}

// ---- the store is not the handler's to hide --------------------------------------

func TestWithoutAStoreThePolicyEndpointsAnswer503(t *testing.T) {
	router := routerWith(nil)
	for _, method := range []string{http.MethodGet, http.MethodPut} {
		got := call(t, router, method, policyPath("org-a"), "operator-a", offBody)
		if got.Code != http.StatusServiceUnavailable || errorCode(got) != "policy_store_unavailable" {
			t.Fatalf("%s without a store = %d %s, want 503", method, got.Code, got.Raw)
		}
	}
}

func TestAMissingPolicyTableIsA503AndOtherFailuresAre500WithoutDetail(t *testing.T) {
	store := newFakeStore()
	router := routerWith(store)

	store.err = fmt.Errorf("%w: relation does not exist", policy.ErrNotMigrated)
	got := call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", offBody)
	if got.Code != http.StatusServiceUnavailable || errorCode(got) != "policy_store_unavailable" {
		t.Fatalf("not migrated = %d %s, want 503", got.Code, got.Raw)
	}

	store.err = errors.New(`ERROR: deadlock detected (SQLSTATE 40P01) on org_processing_policy`)
	got = call(t, router, http.MethodPut, policyPath("org-a"), "operator-a", offBody)
	if got.Code != http.StatusInternalServerError || strings.Contains(got.Raw, "deadlock") || strings.Contains(got.Raw, "SQLSTATE") {
		t.Fatalf("store failure = %d %s, want 500 and no database detail in the body", got.Code, got.Raw)
	}
	got = call(t, router, http.MethodGet, policyPath("org-a"), "operator-a", "")
	if got.Code != http.StatusInternalServerError || strings.Contains(got.Raw, "deadlock") {
		t.Fatalf("store read failure = %d %s", got.Code, got.Raw)
	}
}

// enforceEverything makes every part of the policy enforced for one test, as it will be once the budget
// gate (CR-T9b) reads its columns too (graph-index, CR-T12, already reads graph_extraction). These tests
// do not run in parallel.
func enforceEverything(t *testing.T) {
	t.Helper()
	before := policy.Enforced
	policy.Enforced = policy.Enforcement{GraphExtraction: true, Budget: true}
	t.Cleanup(func() { policy.Enforced = before })
}
