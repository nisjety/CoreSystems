package authctx

import (
	"context"
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
	"google.golang.org/grpc"
	"google.golang.org/grpc/metadata"
)

// INNH-10 MP-1: the api_key principal.

func keyVerifier(t *testing.T, accept bool) (*Verifier, *rsa.PrivateKey) {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatal(err)
	}
	verifier, err := NewVerifier(Config{
		Audiences:    []string{"browser-broker"},
		Issuer:       testIssuer,
		PublicKeyPEM: pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der}),
		AcceptAPIKey: accept,
	})
	if err != nil {
		t.Fatal(err)
	}
	return verifier, key
}

// keyClaims is a valid api_key token; each case mutates one claim.
func keyClaims() jwt.MapClaims {
	now := time.Now()
	return jwt.MapClaims{
		"iss":            testIssuer,
		"aud":            []string{"browser-broker"},
		"sub":            "apikey:key_123",
		"principal_type": "api_key",
		"key_id":         "key_123",
		"acting_user_id": "user-a",
		"org_id":         "org-a",
		"scopes":         []string{"browser:agent"},
		"zdr":            true,
		"key_hosts":      []string{"example.com", "docs.example.com"},
		"key_budget":     map[string]any{"micro_usd": 1_000_000, "max_steps": 50, "max_runtime_s": 600},
		"iat":            now.Add(-time.Minute).Unix(),
		"nbf":            now.Add(-time.Minute).Unix(),
		"exp":            now.Add(2 * time.Minute).Unix(),
	}
}

func signMap(t *testing.T, key *rsa.PrivateKey, claims jwt.MapClaims) string {
	t.Helper()
	raw, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatal(err)
	}
	return raw
}

func TestVerify_AcceptsTheExactAPIKeyShapeWhenOptedIn(t *testing.T) {
	verifier, key := keyVerifier(t, true)
	principal, err := verifier.Verify(signMap(t, key, keyClaims()))
	if err != nil {
		t.Fatal(err)
	}
	if principal.PrincipalType != PrincipalTypeAPIKey || principal.KeyID != "key_123" ||
		principal.ActorID != "user-a" || principal.OrganizationID != "org-a" {
		t.Fatalf("principal = %+v", principal)
	}
	if !principal.ZeroDataRetention || !principal.HasScope("browser:agent") {
		t.Fatalf("zdr/scope lost: %+v", principal)
	}
	if strings.Join(principal.KeyHostCeiling, ",") != "example.com,docs.example.com" {
		t.Fatalf("host ceiling = %v", principal.KeyHostCeiling)
	}
	if principal.KeyBudgetCeiling != (KeyBudget{MicroUSD: 1_000_000, MaxSteps: 50, MaxRuntimeS: 600}) {
		t.Fatalf("budget ceiling = %+v", principal.KeyBudgetCeiling)
	}
}

// Every service that does not opt in (bridge-core, capability-core, cost-core,
// letta-bridge, orchestrator-core, sandbox-manager) keeps refusing the type.
func TestVerify_RefusesAPIKeyByDefault(t *testing.T) {
	verifier, key := keyVerifier(t, false)
	if _, err := verifier.Verify(signMap(t, key, keyClaims())); err == nil {
		t.Fatal("a verifier that did not opt in accepted an api_key token")
	}
}

func TestVerify_RefusesEveryOtherAPIKeyShape(t *testing.T) {
	verifier, key := keyVerifier(t, true)
	cases := map[string]func(jwt.MapClaims){
		"missing key_id":            func(c jwt.MapClaims) { delete(c, "key_id") },
		"empty key_id":              func(c jwt.MapClaims) { c["key_id"] = ""; c["sub"] = "apikey:" },
		"key_id over 128 bytes":     func(c jwt.MapClaims) { id := strings.Repeat("k", 129); c["key_id"] = id; c["sub"] = "apikey:" + id },
		"key_id with a space":       func(c jwt.MapClaims) { c["key_id"] = "key 1"; c["sub"] = "apikey:key 1" },
		"key_id with a control":     func(c jwt.MapClaims) { c["key_id"] = "key\x01"; c["sub"] = "apikey:key\x01" },
		"key_id non-ascii":          func(c jwt.MapClaims) { c["key_id"] = "kéy"; c["sub"] = "apikey:kéy" },
		"sub of another key":        func(c jwt.MapClaims) { c["sub"] = "apikey:key_999" },
		"sub without prefix":        func(c jwt.MapClaims) { c["sub"] = "key_123" },
		"sub is the owner":          func(c jwt.MapClaims) { c["sub"] = "user-a" },
		"sub padded":                func(c jwt.MapClaims) { c["sub"] = " apikey:key_123" },
		"missing acting_user_id":    func(c jwt.MapClaims) { delete(c, "acting_user_id") },
		"blank acting_user_id":      func(c jwt.MapClaims) { c["acting_user_id"] = "  " },
		"padded acting_user_id":     func(c jwt.MapClaims) { c["acting_user_id"] = "user-a " },
		"owner is a key":            func(c jwt.MapClaims) { c["acting_user_id"] = "apikey:key_9" },
		"user_id present":           func(c jwt.MapClaims) { c["user_id"] = "user-a" },
		"service_id present":        func(c jwt.MapClaims) { c["service_id"] = "service:x" },
		"missing org":               func(c jwt.MapClaims) { delete(c, "org_id") },
		"too many hosts":            func(c jwt.MapClaims) { c["key_hosts"] = make([]string, 33) },
		"empty host":                func(c jwt.MapClaims) { c["key_hosts"] = []string{""} },
		"negative budget ceiling":   func(c jwt.MapClaims) { c["key_budget"] = map[string]any{"micro_usd": -1} },
		"wrong audience":            func(c jwt.MapClaims) { c["aud"] = []string{"quarry"} },
		"expired":                   func(c jwt.MapClaims) { c["exp"] = time.Now().Add(-time.Hour).Unix() },
		"type in another case":      func(c jwt.MapClaims) { c["principal_type"] = "API_KEY" },
		"type with suffix":          func(c jwt.MapClaims) { c["principal_type"] = "api_key_v2" },
		"type missing (key fields)": func(c jwt.MapClaims) { delete(c, "principal_type") },
	}
	for name, mutate := range cases {
		t.Run(name, func(t *testing.T) {
			claims := keyClaims()
			mutate(claims)
			if p, err := verifier.Verify(signMap(t, key, claims)); err == nil {
				t.Fatalf("accepted: %+v", p)
			}
		})
	}
}

// key_id on a user or service token is refused as ambiguous, so no legacy type
// can carry a key's identity.
func TestVerify_KeyIDOnAUserOrServiceTokenIsRefused(t *testing.T) {
	verifier, key := keyVerifier(t, true)
	user := keyClaims()
	user["principal_type"], user["user_id"], user["sub"] = "user", "user-a", "user-a"
	delete(user, "acting_user_id")
	service := keyClaims()
	service["principal_type"], service["service_id"], service["sub"] = "service", "service:gw", "service:gw"
	for name, claims := range map[string]jwt.MapClaims{"user": user, "service": service} {
		if _, err := verifier.Verify(signMap(t, key, claims)); err == nil {
			t.Fatalf("%s token with key_id accepted", name)
		}
		delete(claims, "key_id")
		p, err := verifier.Verify(signMap(t, key, claims))
		if err != nil {
			t.Fatalf("%s token without key_id: %v", name, err)
		}
		if p.KeyID != "" || len(p.KeyHostCeiling) != 0 || p.KeyBudgetCeiling != (KeyBudget{}) {
			t.Fatalf("%s token picked up key claims: %+v", name, p)
		}
	}
}

func TestAPIKeyPrincipal_ContextCopiesTheHostCeiling(t *testing.T) {
	p := Principal{OrganizationID: "org-a", ActorID: "user-a", PrincipalType: PrincipalTypeAPIKey, KeyID: "k", KeyHostCeiling: []string{"a.example"}}
	ctx := ContextWithPrincipal(context.Background(), p)
	p.KeyHostCeiling[0] = "evil.example"
	got, ok := PrincipalFromContext(ctx)
	if !ok || got.KeyHostCeiling[0] != "a.example" || got.KeyID != "k" {
		t.Fatalf("context principal = %+v", got)
	}
	got.KeyHostCeiling[0] = "evil.example"
	again, _ := PrincipalFromContext(ctx)
	if again.KeyHostCeiling[0] != "a.example" {
		t.Fatal("a reader mutated the stored host ceiling")
	}
}

// The gRPC interceptor carries the key principal; the HTTP middleware refuses
// it (it would forward the owner as X-User-ID).
func TestAPIKeyPrincipal_GRPCOnly(t *testing.T) {
	verifier, key := keyVerifier(t, true)
	raw := signMap(t, key, keyClaims())

	interceptor := verifier.UnaryServerInterceptor(nil)
	ctx := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+raw))
	_, err := interceptor(ctx, nil, &grpc.UnaryServerInfo{FullMethod: "/x/Y"}, func(ctx context.Context, _ any) (any, error) {
		p, ok := PrincipalFromContext(ctx)
		if !ok || p.KeyID != "key_123" || p.ActorID != "user-a" {
			t.Fatalf("principal = %+v", p)
		}
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}

	called := false
	handler := verifier.HTTPMiddleware(nil)(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { called = true }))
	request := httptest.NewRequest(http.MethodGet, "/", nil)
	request.Header.Set("Authorization", "Bearer "+raw)
	recorder := httptest.NewRecorder()
	handler.ServeHTTP(recorder, request)
	if called || recorder.Code != http.StatusForbidden {
		t.Fatalf("HTTP middleware let an api_key through: code %d called %v", recorder.Code, called)
	}
}
