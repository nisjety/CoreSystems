package handler

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/pem"
	"net/http"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/golang-jwt/jwt/v5"

	"github.com/triodelab/dataplane/services/documents-api-go/pkg/authctx"
)

// Shared helpers for the WS14 handler tests (room files, shares). They drive
// the REAL authctx middleware with a throwaway RSA key, so the claims and the
// forwarded Authorization header a handler sees are exactly what production
// verification produces; nothing here fakes a "verified" flag.

const (
	testJWTAudience = "data-plane"
	testJWTIssuer   = "http://localhost:3011/api/convex-auth"
)

type testAuth struct {
	t   *testing.T
	key *rsa.PrivateKey
	mw  func(http.Handler) http.Handler
}

func newTestAuth(t *testing.T) *testAuth {
	t.Helper()
	key, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	der, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		t.Fatalf("marshal public key: %v", err)
	}
	path := filepath.Join(t.TempDir(), "jwt.pub")
	if err := os.WriteFile(path, pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: der}), 0o600); err != nil {
		t.Fatalf("write public key: %v", err)
	}
	t.Setenv("JWT_PUBLIC_KEY_FILE", path)
	t.Setenv("AUTH_CORE_JWKS_URL", "")
	t.Setenv("JWT_JWKS_URL", "")
	enforce := true
	return &testAuth{t: t, key: key, mw: authctx.Middleware(authctx.Config{Audience: testJWTAudience, ExpectedIssuer: testJWTIssuer, Enforce: &enforce})}
}

func (a *testAuth) sign(claims jwt.MapClaims) string {
	a.t.Helper()
	now := time.Now()
	claims["aud"] = testJWTAudience
	claims["iss"] = testJWTIssuer
	claims["iat"] = now.Add(-time.Minute).Unix()
	claims["nbf"] = now.Add(-time.Minute).Unix()
	claims["exp"] = now.Add(time.Hour).Unix()
	signed, err := jwt.NewWithClaims(jwt.SigningMethodRS256, claims).SignedString(a.key)
	if err != nil {
		a.t.Fatalf("sign token: %v", err)
	}
	return signed
}

// userToken is a verified end-user token. zdr is the signed retention posture.
func (a *testAuth) userToken(org, user string, zdr bool, scopes ...string) string {
	claims := jwt.MapClaims{"user_id": user, "sub": user, "org_id": org, "principal_type": "user", "zdr": zdr}
	if len(scopes) > 0 {
		claims["scopes"] = scopes
	}
	return a.sign(claims)
}

func (a *testAuth) serviceToken(org, service string, scopes ...string) string {
	return a.sign(jwt.MapClaims{"service_id": service, "sub": service, "org_id": org, "principal_type": "service", "zdr": false, "scopes": scopes})
}

func (a *testAuth) serviceTokenZDR(org, service string, zdr bool, scopes ...string) string {
	return a.sign(jwt.MapClaims{"service_id": service, "sub": service, "org_id": org, "principal_type": "service", "zdr": zdr, "scopes": scopes})
}

// wrap puts verification and the org middleware in front of h, like main does.
func (a *testAuth) wrap(h http.Handler) http.Handler {
	return a.mw(OrgIDMiddleware(h))
}

// ── Control Space import decisions ───────────────────────────────────────────

type decisionSigner struct {
	t   *testing.T
	key ed25519.PrivateKey
	id  string
}

// newDecisionSigner installs a throwaway Control decision key in the env.
func newDecisionSigner(t *testing.T) *decisionSigner {
	t.Helper()
	publicKey, privateKey, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	t.Setenv("CONTROL_SPACE_DECISION_KEY_ID", "control-key-1")
	t.Setenv("CONTROL_SPACE_DECISION_PUBLIC_KEY_BASE64", base64.RawURLEncoding.EncodeToString(publicKey))
	t.Setenv("CONTROL_SPACE_DECISION_PUBLIC_KEYS_JSON", "")
	return &decisionSigner{t: t, key: privateKey, id: "control-key-1"}
}

// decision returns a valid space_upload decision for (org, space, subject).
func baseDecision(org, space, subject, sourceType string) spaceImportDecisionClaims {
	now := time.Now().UTC()
	return spaceImportDecisionClaims{
		DecisionRef: "decision-1", OrgID: org, SpaceRef: space, SubjectID: subject,
		ServiceAudience: spaceImportDecisionAudience, ActionID: spaceImportDecisionAction,
		ActionSchemaHash: spaceImportDecisionSchema, PayloadDigest: "sha256:payload", IdempotencyKey: "upload-1",
		RecipientAudienceRef: "audience:" + subject, PrivacyPolicyRef: "privacy:" + org, ResourceAuthorizationRef: "resource:upload-1",
		AuthorityRevision: 1, MembershipRevision: 1, PrivacyRevision: 1, RecipientAudienceRevision: 1, EntitlementRevision: 1,
		Permissions: []string{"documents:write"}, Purpose: "room_file", LawfulBasis: "contract", PrivacyClass: "internal",
		RetentionClass: "standard", Residency: "eu-north-1", DeletionScope: "space", ImportSourceType: sourceType,
		IssuedAt: now, ExpiresAt: now.Add(time.Minute), Nonce: "nonce-1",
	}
}

func (s *decisionSigner) sign(decision spaceImportDecisionClaims) string {
	s.t.Helper()
	return signedSpaceImportDecision(s.t, s.key, s.id, decision)
}
