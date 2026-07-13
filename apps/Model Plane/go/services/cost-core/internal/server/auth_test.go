package server

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/triodelab/model-plane/pkg/authctx"
	"github.com/triodelab/model-plane/services/cost-core/internal/ledger"
)

const costTestIssuer = "https://auth.example.test/api/convex-auth"

type costTestClaims struct {
	OrgID         string   `json:"org_id"`
	UserID        string   `json:"user_id,omitempty"`
	ServiceID     string   `json:"service_id,omitempty"`
	PrincipalType string   `json:"principal_type"`
	Scopes        []string `json:"scopes,omitempty"`
	jwt.RegisteredClaims
}

func protectedCostServer(t *testing.T) (http.Handler, *rsa.PrivateKey) {
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
		Audiences: []string{"cost-core"},
		Issuer:    costTestIssuer,
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{
			Type:  "PUBLIC KEY",
			Bytes: publicDER,
		}),
	})
	if err != nil {
		t.Fatal(err)
	}
	srv := NewServer(ledger.NewStore())
	return srv.Handler(verifier.HTTPMiddleware(CostAuthorizer)), key
}

func costToken(t *testing.T, key *rsa.PrivateKey, claims costTestClaims) string {
	t.Helper()
	now := time.Now()
	claims.Issuer = costTestIssuer
	claims.Audience = jwt.ClaimStrings{"cost-core"}
	claims.IssuedAt = jwt.NewNumericDate(now.Add(-time.Minute))
	claims.NotBefore = jwt.NewNumericDate(now.Add(-time.Minute))
	claims.ExpiresAt = jwt.NewNumericDate(now.Add(time.Hour))
	if claims.PrincipalType == "service" {
		claims.Subject = claims.ServiceID
	} else {
		claims.Subject = claims.UserID
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	raw, err := token.SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func costRequest(handler http.Handler, method, target, body, bearer, orgHeader, userHeader string) *httptest.ResponseRecorder {
	var request *http.Request
	if body == "" {
		request = httptest.NewRequest(method, target, nil)
	} else {
		request = httptest.NewRequest(method, target, strings.NewReader(body))
	}
	if bearer != "" {
		request.Header.Set("Authorization", "Bearer "+bearer)
	}
	if orgHeader != "" {
		request.Header.Set("X-Org-ID", orgHeader)
	}
	if userHeader != "" {
		request.Header.Set("X-User-ID", userHeader)
	}
	response := httptest.NewRecorder()
	handler.ServeHTTP(response, request)
	return response
}

func TestProtectedCostSurfaceRejectsNegativeAuthenticationMatrix(t *testing.T) {
	handler, key := protectedCostServer(t)
	user := costToken(t, key, costTestClaims{
		OrgID: "org-a", UserID: "user-a", PrincipalType: "user",
	})
	tests := []struct {
		name       string
		bearer     string
		orgHeader  string
		userHeader string
		status     int
	}{
		{name: "no auth", status: http.StatusUnauthorized},
		{name: "malformed", bearer: "not-a-jwt", status: http.StatusUnauthorized},
		{name: "forged org header", bearer: user, orgHeader: "org-b", status: http.StatusForbidden},
		{name: "forged user header", bearer: user, userHeader: "user-b", status: http.StatusForbidden},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			response := costRequest(handler, http.MethodGet,
				"/api/v1/usage?org_id=org-a&user_id=user-a", "", test.bearer,
				test.orgHeader, test.userHeader)
			if response.Code != test.status {
				t.Fatalf("status=%d want=%d body=%s", response.Code, test.status, response.Body.String())
			}
		})
	}
	if response := costRequest(handler, http.MethodGet, "/healthz", "", "", "", ""); response.Code != http.StatusOK {
		t.Fatalf("health status=%d", response.Code)
	}
}

func TestCostSurfacePinsTenantAndUserToVerifiedIdentity(t *testing.T) {
	handler, key := protectedCostServer(t)
	service := costToken(t, key, costTestClaims{
		OrgID: "org-a", ServiceID: "service:model-gateway", PrincipalType: "service",
		Scopes: []string{"cost:read", "cost:write"},
	})
	userA := costToken(t, key, costTestClaims{
		OrgID: "org-a", UserID: "user-a", PrincipalType: "user",
	})

	record := `{"org_id":"org-a","user_id":"user-a","run_id":"run-a","cost_usd":2.5}`
	if response := costRequest(handler, http.MethodPost, "/api/v1/cost/record", record, service, "", ""); response.Code != http.StatusAccepted {
		t.Fatalf("service record status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodGet, "/api/v1/usage?org_id=org-a&user_id=user-a", "", userA, "", ""); response.Code != http.StatusOK {
		t.Fatalf("own usage status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodGet, "/api/v1/usage?org_id=org-b&user_id=user-a", "", userA, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("wrong tenant status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodGet, "/api/v1/usage?org_id=org-a&user_id=user-b", "", userA, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("wrong user status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodPost, "/api/v1/cost/record", record, userA, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("user write status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org-a","user_id":"user-b","max_cost_usd":10}`, userA, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("wrong-user budget status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodPost, "/api/v1/budget/check", `{"org_id":"org-a","user_id":"user-a","max_cost_usd":10}`, userA, "", ""); response.Code != http.StatusOK {
		t.Fatalf("own budget status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodGet, "/api/v1/cost/run?org_id=org-b&run_id=run-a", "", service, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("cross-tenant run status=%d body=%s", response.Code, response.Body.String())
	}
}

func TestCostSurfaceRequiresServiceScopesForOrgWideOperations(t *testing.T) {
	handler, key := protectedCostServer(t)
	readOnly := costToken(t, key, costTestClaims{
		OrgID: "org-a", ServiceID: "service:frontend-gateway", PrincipalType: "service",
		Scopes: []string{"cost:read"},
	})
	withoutScope := costToken(t, key, costTestClaims{
		OrgID: "org-a", ServiceID: "service:untrusted", PrincipalType: "service",
		Scopes: []string{"other:read"},
	})
	if response := costRequest(handler, http.MethodGet, "/api/v1/cost/entries?org_id=org-a", "", readOnly, "", ""); response.Code != http.StatusOK {
		t.Fatalf("scoped read status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodGet, "/api/v1/cost/entries?org_id=org-a", "", withoutScope, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("unscoped read status=%d body=%s", response.Code, response.Body.String())
	}
	if response := costRequest(handler, http.MethodPost, "/api/v1/cost/record", `{"org_id":"org-a"}`, readOnly, "", ""); response.Code != http.StatusForbidden {
		t.Fatalf("read-only write status=%d body=%s", response.Code, response.Body.String())
	}
}
