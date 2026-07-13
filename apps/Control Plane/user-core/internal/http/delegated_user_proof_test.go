package http

import (
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/pem"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/golang-jwt/jwt/v5"
)

func TestPlaneUserVerifierPinsVerifiedUserAndTenant(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: mustMarshalPublicKey(t, &privateKey.PublicKey)})
	verifier, err := newPlaneUserVerifier(publicPEM, "https://auth.example/issuer", "data-plane")
	if err != nil {
		t.Fatal(err)
	}

	now := time.Now().UTC()
	claims := planeUserClaims{
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer: "https://auth.example/issuer", Subject: "user-1",
			Audience: jwt.ClaimStrings{"data-plane"}, IssuedAt: jwt.NewNumericDate(now),
			NotBefore: jwt.NewNumericDate(now.Add(-time.Second)), ExpiresAt: jwt.NewNumericDate(now.Add(time.Minute)),
		},
		UserID: "user-1", OrgID: "org-1", PrincipalType: "user",
	}
	token := jwt.NewWithClaims(jwt.SigningMethodRS256, claims)
	signed, err := token.SignedString(privateKey)
	if err != nil {
		t.Fatal(err)
	}
	proof, err := verifier.VerifyAuthorization("Bearer " + signed)
	if err != nil || proof.UserID != "user-1" || proof.OrgID != "org-1" {
		t.Fatalf("proof=%+v err=%v", proof, err)
	}

	unsigned := jwt.NewWithClaims(jwt.SigningMethodNone, claims)
	unsignedToken, err := unsigned.SignedString(jwt.UnsafeAllowNoneSignatureType)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := verifier.VerifyAuthorization("Bearer " + unsignedToken); err == nil {
		t.Fatal("unsigned user proof was accepted")
	}
	for _, malformed := range []string{"", "Basic token", "Bearer ", "Bearer token with-space", "Bearer token\r\nX-Forged: value"} {
		if _, err := verifier.VerifyAuthorization(malformed); err == nil {
			t.Fatalf("malformed authorization %q was accepted", malformed)
		}
	}
}

func TestPlaneUserVerifierConfigurationAndClaimsFailClosed(t *testing.T) {
	if _, err := newPlaneUserVerifier(nil, "issuer", "data-plane"); err == nil {
		t.Fatal("missing public key was accepted")
	}
	if _, err := newPlaneUserVerifier([]byte("not a public key"), "issuer", "data-plane"); err == nil {
		t.Fatal("malformed public key was accepted")
	}
	t.Setenv("AUTH_CORE_JWT_PUBLIC_KEY_FILE", "")
	if _, err := planeUserVerifierFromEnv(); err == nil {
		t.Fatal("missing verifier environment was accepted")
	}
	t.Setenv("AUTH_CORE_JWT_PUBLIC_KEY_FILE", filepath.Join(t.TempDir(), "missing.pub"))
	if _, err := planeUserVerifierFromEnv(); err == nil {
		t.Fatal("unreadable verifier key was accepted")
	}

	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: mustMarshalPublicKey(t, &privateKey.PublicKey)})
	verifier, err := newPlaneUserVerifier(publicPEM, "issuer", "data-plane")
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC()
	claims := planeUserClaims{
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer: "issuer", Subject: "user-1", Audience: jwt.ClaimStrings{"data-plane"},
			IssuedAt: jwt.NewNumericDate(now), NotBefore: jwt.NewNumericDate(now), ExpiresAt: jwt.NewNumericDate(now.Add(time.Minute)),
		},
		UserID: "different-user", OrgID: "org-1", PrincipalType: "user",
	}
	signed, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(privateKey)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := verifier.VerifyAuthorization("Bearer " + signed); err == nil {
		t.Fatal("ambiguous user identity was accepted")
	}
}

func TestV2ServiceDelegationRequiresMatchingUserBearerAndRejectsReplay(t *testing.T) {
	privateKey, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	publicPEM := pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: mustMarshalPublicKey(t, &privateKey.PublicKey)})
	publicPath := filepath.Join(t.TempDir(), "auth-core.pub")
	if err := os.WriteFile(publicPath, publicPEM, 0o600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("AUTH_CORE_JWT_PUBLIC_KEY_FILE", publicPath)
	t.Setenv("AUTH_CORE_ISSUER", "https://auth.example/issuer")
	t.Setenv("DATA_PLANE_AUTH_AUDIENCE", "data-plane")
	t.Setenv("USER_CORE_SERVICE_CREDENTIALS", `[{"principal":"retrieval-engine","audience":"user-core","token":"0123456789abcdef0123456789abcdef","scopes":["authz:read"]}]`)

	now := time.Now().UTC()
	claims := planeUserClaims{
		RegisteredClaims: jwt.RegisteredClaims{
			Issuer: "https://auth.example/issuer", Subject: "user-1", Audience: jwt.ClaimStrings{"data-plane"},
			IssuedAt: jwt.NewNumericDate(now), NotBefore: jwt.NewNumericDate(now.Add(-time.Second)), ExpiresAt: jwt.NewNumericDate(now.Add(time.Minute)),
		},
		UserID: "user-1", OrgID: "org-1", PrincipalType: "user",
	}
	token, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(privateKey)
	if err != nil {
		t.Fatal(err)
	}

	router := gin.New()
	router.Use(authContextMiddleware())
	router.GET("/api/v1/internal/authz/visible", func(c *gin.Context) {
		if !c.GetBool("delegated_user_proof_verified") {
			c.Status(http.StatusForbidden)
			return
		}
		c.Status(http.StatusOK)
	})

	makeRequest := func(authorization string) *http.Request {
		request := httptest.NewRequest(http.MethodGet, "/api/v1/internal/authz/visible?org_id=org-1&resource_type=document&subject_id=user-1", nil)
		signTestAuthzDelegation(request, "retrieval-engine", "user-1", "org-1", "authz:visible", "document", "", "resolve explicit grants", nil, now)
		if authorization != "" {
			request.Header.Set("Authorization", authorization)
		}
		return request
	}

	missing := httptest.NewRecorder()
	router.ServeHTTP(missing, makeRequest(""))
	if missing.Code != http.StatusForbidden {
		t.Fatalf("missing proof status=%d", missing.Code)
	}
	valid := httptest.NewRecorder()
	router.ServeHTTP(valid, makeRequest("Bearer "+token))
	if valid.Code != http.StatusOK {
		t.Fatalf("valid proof status=%d body=%s", valid.Code, valid.Body.String())
	}
	replay := httptest.NewRecorder()
	router.ServeHTTP(replay, makeRequest("Bearer "+token))
	if replay.Code != http.StatusForbidden {
		t.Fatalf("replay status=%d", replay.Code)
	}
}

func TestDelegationNonceCacheConsumesNonceOnce(t *testing.T) {
	cache := newDelegationNonceCache(2)
	now := time.Now().UTC()
	if !cache.Consume("service:nonce", now.Add(time.Minute), now) {
		t.Fatal("first nonce use was rejected")
	}
	if cache.Consume("service:nonce", now.Add(time.Minute), now) {
		t.Fatal("replayed nonce was accepted")
	}
}

func mustMarshalPublicKey(t *testing.T, key *rsa.PublicKey) []byte {
	t.Helper()
	der, err := x509.MarshalPKIXPublicKey(key)
	if err != nil {
		t.Fatal(err)
	}
	return der
}
