package server

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"net/http"
	"testing"

	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// F-f10-23a: POST /api/v1/budget/check for a service principal needs the
// cost:budget:check scope, and no other scope admits it. A user's own budget check
// is unchanged, and an api_key principal reaches that one route and nothing else.

// budgetScopeServer is the protected cost-core surface over a ledger holding the
// spend of two users in org-a (3 and 4) and one user in org-b (50).
func budgetScopeServer(t *testing.T) (http.Handler, *rsa.PrivateKey) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicDER, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := authctx.NewVerifier(authctx.Config{
		Audiences:    []string{"cost-core"},
		Issuer:       costTestIssuer,
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER}),
	})
	if err != nil {
		t.Fatal(err)
	}
	store := ledger.NewStore()
	for _, entry := range []ledger.Entry{
		{OrgID: "org-a", UserID: "user-a", RunID: "r1", CostUSD: 3},
		{OrgID: "org-a", UserID: "user-b", RunID: "r2", CostUSD: 4},
		{OrgID: "org-b", UserID: "user-c", RunID: "r3", CostUSD: 50},
	} {
		if err := store.RecordEntry(context.Background(), entry); err != nil {
			t.Fatal(err)
		}
	}
	return NewServer(store).Handler(verifier.HTTPMiddleware(CostAuthorizer)), key
}

func TestBudgetCheckServiceNeedsCostBudgetCheckAndIsPinnedToItsOrg(t *testing.T) {
	handler, key := budgetScopeServer(t)
	budget := costToken(t, key, costTestClaims{
		OrgID: "org-a", ServiceID: "service:model-gateway", PrincipalType: "service",
		Scopes: []string{ScopeBudgetCheck},
	})

	// Without user_id the service asks the org-wide question: 3 + 4 in org-a, not
	// org-b's 50.
	org := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":100}`, budget, "", "")
	if org.Code != http.StatusOK {
		t.Fatalf("org-wide budget check status=%d body=%s", org.Code, org.Body.String())
	}
	if got := decodeBudget(t, org.Body.Bytes()); !got.Allowed || got.CurrentCostUSD != 7 {
		t.Fatalf("org-wide budget check = %+v, want allowed at 7", got)
	}

	// A user_id the service names stays as sent: the service is not pinned to
	// its own id, so this is user-a's spend.
	one := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"user_id":"user-a","max_cost_usd":100}`, budget, "", "")
	if one.Code != http.StatusOK || decodeBudget(t, one.Body.Bytes()).CurrentCostUSD != 3 {
		t.Fatalf("per-user budget check status=%d body=%s", one.Code, one.Body.String())
	}

	// The organization is pinned to the verified token.
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org-b","max_cost_usd":100}`, budget, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("another organization status=%d body=%s", response.Code, response.Body.String())
	}
}

func TestBudgetCheckRefusesAServiceWithoutCostBudgetCheck(t *testing.T) {
	handler, key := budgetScopeServer(t)
	for name, claims := range map[string]costTestClaims{
		// The Data Plane forwarder holds cost:write alone: it may record, not check.
		"the data orchestrator (cost:write only)": {
			OrgID: "org-a", ServiceID: "service:data-orchestrator", PrincipalType: "service",
			Scopes: []string{"cost:write"},
		},
		// A read service no longer checks a budget: the check needs its own scope.
		"a read service (cost:read only)": {
			OrgID: "org-a", ServiceID: "service:frontend-gateway", PrincipalType: "service",
			Scopes: []string{"cost:read"},
		},
		"a service with no cost scope": {
			OrgID: "org-a", ServiceID: "service:untrusted", PrincipalType: "service",
			Scopes: []string{"other:read"},
		},
	} {
		token := costToken(t, key, claims)
		response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":100}`, token, "", "")
		if response.Code != http.StatusForbidden {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
}

// cost:budget:check is the budget check's scope alone: it opens no ledger write
// and no read.
func TestBudgetCheckScopeOpensNoOtherCostRoute(t *testing.T) {
	handler, key := budgetScopeServer(t)
	budget := costToken(t, key, costTestClaims{
		OrgID: "org-a", ServiceID: "service:model-gateway", PrincipalType: "service",
		Scopes: []string{ScopeBudgetCheck},
	})
	for name, call := range map[string]struct{ method, target, body string }{
		"record":    {http.MethodPost, "/api/v1/cost/record", `{"org_id":"org-a","user_id":"user-a","cost_usd":1}`},
		"usage":     {http.MethodGet, "/api/v1/usage?org_id=org-a&user_id=user-a", ""},
		"run":       {http.MethodGet, "/api/v1/cost/run?org_id=org-a&run_id=r1", ""},
		"aggregate": {http.MethodGet, "/api/v1/cost/aggregate?org_id=org-a", ""},
		"users":     {http.MethodGet, "/api/v1/cost/users?org_id=org-a", ""},
		"entries":   {http.MethodGet, "/api/v1/cost/entries?org_id=org-a", ""},
	} {
		response := costRequest(handler, call.method, call.target, call.body, budget, "", "")
		if response.Code != http.StatusForbidden {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
}

// A user's own budget check is unchanged: the check is pinned to the user's own
// spend, and the user principal still reaches no ledger write.
func TestBudgetCheckUserPrincipalIsUnchanged(t *testing.T) {
	handler, key := budgetScopeServer(t)
	user := costToken(t, key, costTestClaims{OrgID: "org-a", UserID: "user-a", PrincipalType: "user"})

	own := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":100}`, user, "", "")
	if own.Code != http.StatusOK || decodeBudget(t, own.Body.Bytes()).CurrentCostUSD != 3 {
		t.Fatalf("own budget check status=%d body=%s", own.Code, own.Body.String())
	}
	for name, body := range map[string]string{
		"another user":         `{"user_id":"user-b","max_cost_usd":100}`,
		"another organization": `{"org_id":"org-b","max_cost_usd":100}`,
	} {
		if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", body, user, "", ""); response.Code != http.StatusForbidden {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
	if response := costRequest(handler, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org-a","cost_usd":1}`, user, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("user write status=%d body=%s", response.Code, response.Body.String())
	}
}

// An api_key principal reaches only POST /api/v1/budget/check. The same path under
// another method, and an unlisted path, are refused too. The public pricing catalogue
// sits outside the middleware (see TestAPIKeyBudgetCheck_CallsNothingElse), so it is
// not part of this matrix.
func TestAPIKeyBudgetCheckRefusesTheWrongMethodAndUnlistedPaths(t *testing.T) {
	handler, key := keyBudgetServer(t, true)
	token := keyBudgetToken(t, key, nil)
	for name, call := range map[string]struct{ method, target string }{
		"budget check by GET":   {http.MethodGet, "/api/v1/budget/check"},
		"budget check by PUT":   {http.MethodPut, "/api/v1/budget/check"},
		"an unlisted cost path": {http.MethodPost, "/api/v1/cost/unlisted"},
	} {
		response := costRequest(handler, call.method, call.target, `{"max_cost_usd":10}`, token, "", "")
		if response.Code != http.StatusForbidden {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
}
