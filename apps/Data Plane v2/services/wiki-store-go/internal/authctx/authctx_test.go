package authctx

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"encoding/pem"
	"math/big"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

const (
	testAudience = "data-plane"
	testIssuer   = "https://auth.test/issuer"
)

func TestHTTPMiddlewareEnforcesSignedFourShapeMatrix(t *testing.T) {
	key := generateTestKey(t)
	verifier := newTestVerifier(t, key)
	validBearer := signTestToken(t, key, validTestClaims())

	tests := []struct {
		name       string
		bearer     string
		headerOrg  string
		wantStatus int
		wantOrg    string
	}{
		{name: "no auth", wantStatus: http.StatusUnauthorized},
		{name: "forged org header", headerOrg: "org-victim", wantStatus: http.StatusUnauthorized},
		{name: "valid signed bearer", bearer: validBearer, wantStatus: http.StatusNoContent, wantOrg: "org-authorized"},
		{name: "valid bearer plus spoofed org", bearer: validBearer, headerOrg: "org-victim", wantStatus: http.StatusForbidden},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			handler := Middleware(verifier)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				claims, ok := FromContext(r.Context())
				if !ok {
					t.Fatal("verified claims missing from request context")
				}
				w.Header().Set("X-Seen-Org-ID", claims.OrgID)
				w.WriteHeader(http.StatusNoContent)
			}))
			req := httptest.NewRequest(http.MethodGet, "/v1/wiki/pages", nil)
			if tt.bearer != "" {
				req.Header.Set("Authorization", "Bearer "+tt.bearer)
			}
			if tt.headerOrg != "" {
				req.Header.Set("X-Org-ID", tt.headerOrg)
			}
			response := httptest.NewRecorder()

			handler.ServeHTTP(response, req)

			if response.Code != tt.wantStatus {
				t.Fatalf("status = %d, want %d; body=%s", response.Code, tt.wantStatus, response.Body.String())
			}
			if got := response.Header().Get("X-Seen-Org-ID"); got != tt.wantOrg {
				t.Fatalf("seen org = %q, want %q", got, tt.wantOrg)
			}
		})
	}
}

func TestVerifierRejectsUnsignedAndInvalidIdentityTokens(t *testing.T) {
	key := generateTestKey(t)
	verifier := newTestVerifier(t, key)

	unsigned := jwt.NewWithClaims(jwt.SigningMethodNone, validTestClaims())
	unsignedToken, err := unsigned.SignedString(jwt.UnsafeAllowNoneSignatureType)
	if err != nil {
		t.Fatalf("sign unsigned token: %v", err)
	}

	tests := []struct {
		name  string
		token string
	}{
		{name: "unsigned", token: unsignedToken},
		{name: "wrong audience", token: signTestToken(t, key, mergeTestClaims(validTestClaims(), jwt.MapClaims{"aud": "other-service"}))},
		{name: "wrong issuer", token: signTestToken(t, key, mergeTestClaims(validTestClaims(), jwt.MapClaims{"iss": "https://evil.test"}))},
		{name: "expired", token: signTestToken(t, key, mergeTestClaims(validTestClaims(), jwt.MapClaims{"exp": time.Now().Add(-time.Minute).Unix()}))},
		{name: "ambiguous user", token: signTestToken(t, key, mergeTestClaims(validTestClaims(), jwt.MapClaims{"sub": "different-user"}))},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := verifier.Verify(tt.token); err == nil {
				t.Fatal("Verify() accepted invalid token")
			}
		})
	}
}

func TestVerifierRequiresSignedBooleanZDRClaim(t *testing.T) {
	key := generateTestKey(t)
	verifier := newTestVerifier(t, key)

	missing := mergeTestClaims(validTestClaims(), jwt.MapClaims{"zdr": nil})
	delete(missing, "zdr")
	for _, tt := range []struct {
		name   string
		claims jwt.MapClaims
	}{
		{name: "missing", claims: missing},
		{name: "null", claims: mergeTestClaims(validTestClaims(), jwt.MapClaims{"zdr": nil})},
		{name: "string", claims: mergeTestClaims(validTestClaims(), jwt.MapClaims{"zdr": "false"})},
		{name: "number", claims: mergeTestClaims(validTestClaims(), jwt.MapClaims{"zdr": 0})},
	} {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := verifier.Verify(signTestToken(t, key, tt.claims)); err == nil {
				t.Fatal("Verify() accepted token without a signed boolean zdr claim")
			}
		})
	}

	for _, want := range []bool{false, true} {
		claims, err := verifier.Verify(signTestToken(t, key, mergeTestClaims(validTestClaims(), jwt.MapClaims{"zdr": want})))
		if err != nil {
			t.Fatalf("Verify(zdr=%v): %v", want, err)
		}
		if claims.RestrictiveZDR() != want {
			t.Fatalf("RestrictiveZDR() = %v, want %v", claims.RestrictiveZDR(), want)
		}
	}
}

func TestNewVerifierFailsClosedWithoutCompleteConfiguration(t *testing.T) {
	key := generateTestKey(t)
	publicPEM := marshalTestPublicKey(t, &key.PublicKey)

	tests := []Config{
		{Issuer: testIssuer, PublicKeyPEM: publicPEM},
		{Audience: testAudience, PublicKeyPEM: publicPEM},
		{Audience: testAudience, Issuer: testIssuer},
	}
	for _, cfg := range tests {
		if _, err := NewVerifier(cfg); err == nil {
			t.Fatalf("NewVerifier(%+v) unexpectedly succeeded", cfg)
		}
	}
}

func TestVerifierUsesJWKSAndCachesTheVerifiedKey(t *testing.T) {
	key := generateTestKey(t)
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		writeTestJWKS(t, w, "wiki-key", &key.PublicKey)
	}))
	defer server.Close()

	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: server.URL})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, validTestClaims())
	token.Header["kid"] = "wiki-key"
	bearer, err := token.SignedString(key)
	if err != nil {
		t.Fatalf("sign JWT: %v", err)
	}

	for range 2 {
		if _, err := verifier.Verify(bearer); err != nil {
			t.Fatalf("Verify with JWKS: %v", err)
		}
	}
	if requests.Load() != 1 {
		t.Fatalf("JWKS requests = %d, want 1", requests.Load())
	}
}

func TestVerifierFailsClosedWhenJWKSIsUnavailable(t *testing.T) {
	key := generateTestKey(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		http.Error(w, "unavailable", http.StatusServiceUnavailable)
	}))
	defer server.Close()
	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: server.URL})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, validTestClaims())
	token.Header["kid"] = "unavailable-key"
	bearer, err := token.SignedString(key)
	if err != nil {
		t.Fatalf("sign JWT: %v", err)
	}
	if _, err := verifier.Verify(bearer); err == nil {
		t.Fatal("Verify accepted token while JWKS was unavailable")
	}
}

func TestVerifierFailsClosedWhenWarmJWKSCacheExpiresDuringOutage(t *testing.T) {
	key := generateTestKey(t)
	var unavailable atomic.Bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		if unavailable.Load() {
			http.Error(w, "unavailable", http.StatusServiceUnavailable)
			return
		}
		writeTestJWKS(t, w, "wiki-key", &key.PublicKey)
	}))
	defer server.Close()
	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, JWKSURL: server.URL})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, validTestClaims())
	token.Header["kid"] = "wiki-key"
	bearer, err := token.SignedString(key)
	if err != nil {
		t.Fatalf("sign JWT: %v", err)
	}
	if _, err := verifier.Verify(bearer); err != nil {
		t.Fatalf("prime JWKS cache: %v", err)
	}
	unavailable.Store(true)
	verifier.jwks.expiresAt = time.Now().Add(-time.Second)
	verifier.jwks.lastAttempt = time.Time{}
	if _, err := verifier.Verify(bearer); err == nil {
		t.Fatal("expired warm JWKS cache was accepted during outage")
	}
}

func TestVerifierSeparatesScopedServicePrincipalFromUserIdentity(t *testing.T) {
	key := generateTestKey(t)
	verifier := newTestVerifier(t, key)
	serviceClaims := mergeTestClaims(validTestClaims(), jwt.MapClaims{
		"sub": "wiki-worker", "user_id": "", "service_id": "wiki-worker",
		"principal_type": "service", "scopes": []string{"wiki.read"},
	})
	claims, err := verifier.Verify(signTestToken(t, key, serviceClaims))
	if err != nil || !claims.IsService() || claims.PrincipalID() != "wiki-worker" {
		t.Fatalf("service claims = (%+v, %v)", claims, err)
	}

	tests := []jwt.MapClaims{
		mergeTestClaims(serviceClaims, jwt.MapClaims{"scopes": []string{}}),
		mergeTestClaims(serviceClaims, jwt.MapClaims{"user_id": "user-too"}),
		mergeTestClaims(serviceClaims, jwt.MapClaims{"sub": "different-service"}),
	}
	for _, invalid := range tests {
		if _, err := verifier.Verify(signTestToken(t, key, invalid)); err == nil {
			t.Fatal("accepted ambiguous or unscoped service principal")
		}
	}
}

func TestRequireScopeAllowsOnlyExplicitlyScopedPrincipals(t *testing.T) {
	next := http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusNoContent) })
	middleware := RequireScope("wiki.write")
	user := &Claims{OrgID: "org", UserID: "user", Scopes: []string{"wiki.write"}, Verified: true}
	userDenied := &Claims{OrgID: "org", UserID: "reader", Scopes: []string{"wiki.read"}, Verified: true}
	serviceAllowed := &Claims{OrgID: "org", ServiceID: "writer", Scopes: []string{"wiki.write"}, Verified: true}
	serviceDenied := &Claims{OrgID: "org", ServiceID: "reader", Scopes: []string{"wiki.read"}, Verified: true}

	for _, tt := range []struct {
		name   string
		claims *Claims
		want   int
	}{{"scoped user", user, http.StatusNoContent}, {"unscoped user", userDenied, http.StatusForbidden}, {"scoped service", serviceAllowed, http.StatusNoContent}, {"unscoped service", serviceDenied, http.StatusForbidden}} {
		t.Run(tt.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodPost, "/v1/wiki/pages", nil)
			req = req.WithContext(context.WithValue(req.Context(), contextKey{}, tt.claims))
			response := httptest.NewRecorder()
			middleware(next).ServeHTTP(response, req)
			if response.Code != tt.want {
				t.Fatalf("status = %d, want %d", response.Code, tt.want)
			}
		})
	}
}

func TestVerifierLoadsStaticPublicKeyFile(t *testing.T) {
	key := generateTestKey(t)
	path := filepath.Join(t.TempDir(), "wiki-public.pem")
	if err := os.WriteFile(path, marshalTestPublicKey(t, &key.PublicKey), 0o600); err != nil {
		t.Fatalf("write test public key: %v", err)
	}
	verifier, err := NewVerifier(Config{Audience: testAudience, Issuer: testIssuer, PublicKeyFile: path})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	if _, err := verifier.Verify(signTestToken(t, key, validTestClaims())); err != nil {
		t.Fatalf("Verify with public-key file: %v", err)
	}
}

func TestTenantIDUsesOnlyVerifiedClaims(t *testing.T) {
	if _, err := TenantID(context.Background(), "org-authorized"); status.Code(err) != codes.Unauthenticated {
		t.Fatalf("missing claims code = %s, want Unauthenticated", status.Code(err))
	}
	claims := &Claims{OrgID: "org-authorized", UserID: "user-authorized", Verified: true}
	ctx := context.WithValue(context.Background(), contextKey{}, claims)
	for _, requested := range []string{"", "org-authorized"} {
		orgID, err := TenantID(ctx, requested)
		if err != nil || orgID != "org-authorized" {
			t.Fatalf("TenantID(%q) = (%q, %v)", requested, orgID, err)
		}
	}
	if _, err := TenantID(ctx, "org-victim"); status.Code(err) != codes.PermissionDenied {
		t.Fatalf("tenant mismatch code = %s, want PermissionDenied", status.Code(err))
	}
}

type testTenantRequest struct{ orgID string }

func (r testTenantRequest) GetOrgId() string { return r.orgID }

func TestUnaryServerInterceptorRejectsAmbiguousOrSpoofedIdentity(t *testing.T) {
	key := generateTestKey(t)
	verifier := newTestVerifier(t, key)
	bearer := signTestToken(t, key, validTestClaims())
	interceptor := UnaryServerInterceptor(verifier)
	handler := func(ctx context.Context, _ any) (any, error) {
		if _, ok := FromContext(ctx); !ok {
			t.Fatal("verified claims missing in gRPC handler")
		}
		return struct{}{}, nil
	}
	info := &grpc.UnaryServerInfo{FullMethod: "/wiki.v1.WikiService/GetPage"}

	tests := []struct {
		name string
		ctx  context.Context
		req  any
		want codes.Code
	}{
		{name: "no auth", ctx: context.Background(), req: testTenantRequest{orgID: "org-authorized"}, want: codes.Unauthenticated},
		{name: "multiple tenant metadata", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+bearer, "x-org-id", "org-authorized", "x-org-id", "org-authorized")), req: testTenantRequest{orgID: "org-authorized"}, want: codes.PermissionDenied},
		{name: "spoofed tenant metadata", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+bearer, "x-org-id", "org-victim")), req: testTenantRequest{orgID: "org-authorized"}, want: codes.PermissionDenied},
		{name: "missing tenant contract", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+bearer)), req: struct{}{}, want: codes.PermissionDenied},
		{name: "spoofed request tenant", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+bearer)), req: testTenantRequest{orgID: "org-victim"}, want: codes.PermissionDenied},
		{name: "valid signed identity", ctx: metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+bearer)), req: testTenantRequest{orgID: "org-authorized"}, want: codes.OK},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := interceptor(tt.ctx, tt.req, info, handler)
			if got := status.Code(err); got != tt.want {
				t.Fatalf("code = %s, want %s; err=%v", got, tt.want, err)
			}
		})
	}
	readOnly := mergeTestClaims(validTestClaims(), jwt.MapClaims{"scopes": []string{"wiki.read"}})
	readOnlyBearer := signTestToken(t, key, readOnly)
	writeCtx := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+readOnlyBearer))
	_, err := interceptor(writeCtx, testTenantRequest{orgID: "org-authorized"}, &grpc.UnaryServerInfo{FullMethod: "/wiki.v1.WikiService/CreatePage"}, handler)
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("read-only gRPC write code = %s, want PermissionDenied", status.Code(err))
	}
	member := mergeTestClaims(validTestClaims(), jwt.MapClaims{"scopes": []string{"wiki.read", "wiki.write"}})
	memberBearer := signTestToken(t, key, member)
	approveCtx := metadata.NewIncomingContext(context.Background(), metadata.Pairs("authorization", "Bearer "+memberBearer))
	_, err = interceptor(approveCtx, testTenantRequest{orgID: "org-authorized"}, &grpc.UnaryServerInfo{FullMethod: "/wiki.v1.WikiService/ReviewProposal"}, handler)
	if status.Code(err) != codes.PermissionDenied {
		t.Fatalf("member gRPC approval code = %s, want PermissionDenied", status.Code(err))
	}
}

func generateTestKey(t *testing.T) *rsa.PrivateKey {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate RSA key: %v", err)
	}
	return key
}

func newTestVerifier(t *testing.T, key *rsa.PrivateKey) *Verifier {
	t.Helper()
	verifier, err := NewVerifier(Config{
		Audience:     testAudience,
		Issuer:       testIssuer,
		PublicKeyPEM: marshalTestPublicKey(t, &key.PublicKey),
	})
	if err != nil {
		t.Fatalf("NewVerifier: %v", err)
	}
	return verifier
}

func marshalTestPublicKey(t *testing.T, key *rsa.PublicKey) []byte {
	t.Helper()
	encoded, err := x509.MarshalPKIXPublicKey(key)
	if err != nil {
		t.Fatalf("marshal RSA public key: %v", err)
	}
	return pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: encoded})
}

func validTestClaims() jwt.MapClaims {
	now := time.Now()
	return jwt.MapClaims{
		"iss": testIssuer, "aud": testAudience,
		"sub": "user-authorized", "user_id": "user-authorized", "org_id": "org-authorized",
		"zdr":    false,
		"scopes": []string{"wiki.read", "wiki.write", "wiki.approve", "wiki.maintenance.write"},
		"iat":    now.Add(-time.Minute).Unix(), "nbf": now.Add(-time.Minute).Unix(), "exp": now.Add(time.Hour).Unix(),
	}
}

func signTestToken(t *testing.T, key *rsa.PrivateKey, claims jwt.MapClaims) string {
	t.Helper()
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(key)
	if err != nil {
		t.Fatalf("sign JWT: %v", err)
	}
	return token
}

func mergeTestClaims(base, overrides jwt.MapClaims) jwt.MapClaims {
	merged := make(jwt.MapClaims, len(base)+len(overrides))
	for key, value := range base {
		merged[key] = value
	}
	for key, value := range overrides {
		merged[key] = value
	}
	return merged
}

func writeTestJWKS(t *testing.T, w http.ResponseWriter, kid string, key *rsa.PublicKey) {
	t.Helper()
	exponent := big.NewInt(int64(key.E)).Bytes()
	document := map[string]any{"keys": []map[string]string{{
		"kid": kid, "kty": "RSA", "use": "sig", "alg": "RS256",
		"n": base64.RawURLEncoding.EncodeToString(key.N.Bytes()),
		"e": base64.RawURLEncoding.EncodeToString(exponent),
	}}}
	w.Header().Set("Content-Type", "application/json")
	if err := json.NewEncoder(w).Encode(document); err != nil {
		t.Errorf("encode JWKS: %v", err)
	}
}
