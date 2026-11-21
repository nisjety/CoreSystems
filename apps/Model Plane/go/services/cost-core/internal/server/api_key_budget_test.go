package server

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"net/http"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

// O-18: cost-core accepts the api_key principal for its own budget check and
// for nothing else.

// keyBudgetServer builds the protected handler the way cmd/main.go does when
// COST_CORE_ACCEPT_API_KEY is on (acceptKey) or off, over a ledger holding the
// key's own spend, a sibling key's spend and the key owner's own spend.
func keyBudgetServer(t *testing.T, acceptKey bool) (http.Handler, *rsa.PrivateKey) {
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
		Audiences:        []string{"cost-core"},
		Issuer:           costTestIssuer,
		PublicKeyPEM:     pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: publicDER}),
		AcceptAPIKey:     acceptKey,
		AcceptAPIKeyHTTP: acceptKey,
	})
	if err != nil {
		t.Fatal(err)
	}
	store := ledger.NewStore()
	for _, entry := range []ledger.Entry{
		{OrgID: "org-a", UserID: "apikey:key-1", RunID: "r1", CostUSD: 2, InputTokens: 100, OutputTokens: 50},
		{OrgID: "org-a", UserID: "apikey:key-2", RunID: "r2", CostUSD: 9, InputTokens: 900, OutputTokens: 90},
		{OrgID: "org-a", UserID: "user-a", RunID: "r3", CostUSD: 7, InputTokens: 700, OutputTokens: 70},
		{OrgID: "org-b", UserID: "apikey:key-1", RunID: "r4", CostUSD: 20, InputTokens: 2000, OutputTokens: 200},
	} {
		if err := store.RecordEntry(context.Background(), entry); err != nil {
			t.Fatal(err)
		}
	}
	return NewServer(store).Handler(verifier.HTTPMiddleware(CostAuthorizer)), key
}

func keyBudgetToken(t *testing.T, key *rsa.PrivateKey, mutate func(jwt.MapClaims)) string {
	t.Helper()
	now := time.Now()
	claims := jwt.MapClaims{
		"iss":            costTestIssuer,
		"aud":            []string{"cost-core"},
		"sub":            "apikey:key-1",
		"principal_type": "api_key",
		"key_id":         "key-1",
		"acting_user_id": "user-a",
		"org_id":         "org-a",
		"scopes":         []string{ScopeBudgetCheck},
		"iat":            now.Add(-time.Minute).Unix(),
		"nbf":            now.Add(-time.Minute).Unix(),
		"exp":            now.Add(2 * time.Minute).Unix(),
	}
	if mutate != nil {
		mutate(claims)
	}
	raw, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func decodeBudget(t *testing.T, body []byte) budgetCheckResponse {
	t.Helper()
	var out budgetCheckResponse
	if err := json.Unmarshal(body, &out); err != nil {
		t.Fatalf("decode %q: %v", body, err)
	}
	return out
}

func TestAPIKeyBudgetCheck_ForcesTheKeyPrincipalAndTheTokenOrg(t *testing.T) {
	handler, key := keyBudgetServer(t, true)
	token := keyBudgetToken(t, key, nil)

	// Neither field sent: the org and the user are the token's own. The key's
	// spend is 2.0 (not the org's 18 and not the owner's 7).
	response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":10}`, token, "", "")
	if response.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
	got := decodeBudget(t, response.Body.Bytes())
	if !got.Allowed || got.CurrentCostUSD != 2 || got.CurrentTokens != 150 {
		t.Fatalf("key spend = %+v, want allowed with cost 2 and 150 tokens", got)
	}

	// The key's own principal named explicitly is the same check.
	explicit := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org-a","user_id":"apikey:key-1","max_cost_usd":10}`, token, "", "")
	if explicit.Code != http.StatusOK || decodeBudget(t, explicit.Body.Bytes()).CurrentCostUSD != 2 {
		t.Fatalf("explicit status=%d body=%s", explicit.Code, explicit.Body.String())
	}

	// A cap below the key's spend is the typed refusal body (allowed false).
	over := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":1}`, token, "", "")
	if over.Code != http.StatusOK {
		t.Fatalf("over status=%d body=%s", over.Code, over.Body.String())
	}
	if refused := decodeBudget(t, over.Body.Bytes()); refused.Allowed || refused.Reason == "" {
		t.Fatalf("over-cap check was allowed: %+v", refused)
	}
}

func TestAPIKeyBudgetCheck_RefusesAnotherOrgUserOrKey(t *testing.T) {
	handler, key := keyBudgetServer(t, true)
	token := keyBudgetToken(t, key, nil)
	for name, body := range map[string]string{
		"another org":               `{"org_id":"org-b","max_cost_usd":10}`,
		"another org naming the id": `{"org_id":"org-b","user_id":"apikey:key-1","max_cost_usd":10}`,
		"the key owner":             `{"org_id":"org-a","user_id":"user-a","max_cost_usd":10}`,
		"a sibling key":             `{"org_id":"org-a","user_id":"apikey:key-2","max_cost_usd":10}`,
	} {
		response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", body, token, "", "")
		if response.Code != http.StatusForbidden {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
}

func TestAPIKeyBudgetCheck_CallsNothingElse(t *testing.T) {
	handler, key := keyBudgetServer(t, true)
	token := keyBudgetToken(t, key, nil)
	// /api/v1/pricing is the public non-tenant catalogue, outside the middleware.
	record := `{"org_id":"org-a","user_id":"apikey:key-1","run_id":"x","cost_usd":1}`
	for name, call := range map[string]struct{ method, target, body string }{
		"record":    {http.MethodPost, "/api/v1/cost/record", record},
		"usage":     {http.MethodGet, "/api/v1/usage?org_id=org-a&user_id=apikey:key-1", ""},
		"run":       {http.MethodGet, "/api/v1/cost/run?org_id=org-a&run_id=r1", ""},
		"aggregate": {http.MethodGet, "/api/v1/cost/aggregate?org_id=org-a", ""},
		"users":     {http.MethodGet, "/api/v1/cost/users?org_id=org-a", ""},
		"entries":   {http.MethodGet, "/api/v1/cost/entries?org_id=org-a", ""},
		"azure":     {http.MethodGet, "/api/v1/cost/azure/actual?org_id=org-a", ""},
		"analytics": {http.MethodGet, "/api/v1/cost/analytics?org_id=org-a", ""},
	} {
		response := costRequest(handler, call.method, call.target, call.body, token, "", "")
		if response.Code != http.StatusForbidden {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
}

func TestAPIKeyBudgetCheck_NeedsTheBudgetScopeAndTheirOwnIdentityHeaders(t *testing.T) {
	handler, key := keyBudgetServer(t, true)

	noScope := keyBudgetToken(t, key, func(c jwt.MapClaims) { c["scopes"] = []string{"agent:runs"} })
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":10}`, noScope, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("a key without cost:budget:check got status=%d body=%s", response.Code, response.Body.String())
	}

	token := keyBudgetToken(t, key, nil)
	// A forged identity header (the owner's id, another org) is refused; the
	// key's own subject is not a conflict.
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":10}`, token, "", "user-a"); response.Code != http.StatusForbidden {
		t.Fatalf("owner header status=%d", response.Code)
	}
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":10}`, token, "org-b", ""); response.Code != http.StatusForbidden {
		t.Fatalf("org header status=%d", response.Code)
	}
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":10}`, token, "org-a", "apikey:key-1"); response.Code != http.StatusOK {
		t.Fatalf("own headers status=%d body=%s", response.Code, response.Body.String())
	}
}

// A ZDR key checks like any other: the request carries ids and caps, no content
// (the ZDR refusal itself is model-gateway's, before any call).
func TestAPIKeyBudgetCheck_ZDRKeyCarriesNoContent(t *testing.T) {
	handler, key := keyBudgetServer(t, true)
	token := keyBudgetToken(t, key, func(c jwt.MapClaims) { c["zdr"] = true })
	response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":10}`, token, "", "")
	if response.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
}

// Off (COST_CORE_ACCEPT_API_KEY unset), the principal type is refused at the
// verifier exactly as before the switch existed.
func TestAPIKeyBudgetCheck_OffIsUnchanged(t *testing.T) {
	handler, key := keyBudgetServer(t, false)
	token := keyBudgetToken(t, key, nil)
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"max_cost_usd":10}`, token, "", ""); response.Code != http.StatusUnauthorized {
		t.Fatalf("off: status=%d body=%s", response.Code, response.Body.String())
	}
}

// Both principals agree: a service with cost:read asking for the key's scope and
// the key principal asking for itself read the same ledger row.
func TestAPIKeyBudgetCheck_AgreesWithTheServiceCheckForTheSamePrincipal(t *testing.T) {
	handler, key := keyBudgetServer(t, true)
	keyToken := keyBudgetToken(t, key, nil)
	service := costToken(t, key, costTestClaims{
		OrgID: "org-a", ServiceID: "service:model-gateway", PrincipalType: "service",
		Scopes: []string{"cost:read"},
	})
	body := `{"org_id":"org-a","user_id":"apikey:key-1","max_cost_usd":10}`
	viaKey := costRequest(handler, http.MethodPost, "/api/v1/budget/check", body, keyToken, "", "")
	viaService := costRequest(handler, http.MethodPost, "/api/v1/budget/check", body, service, "", "")
	if viaKey.Code != http.StatusOK || viaService.Code != http.StatusOK {
		t.Fatalf("key=%d service=%d", viaKey.Code, viaService.Code)
	}
	if decodeBudget(t, viaKey.Body.Bytes()) != decodeBudget(t, viaService.Body.Bytes()) {
		t.Fatalf("paths disagree: key=%s service=%s", viaKey.Body.String(), viaService.Body.String())
	}
}
