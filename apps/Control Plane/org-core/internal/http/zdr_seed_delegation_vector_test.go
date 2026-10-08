package http

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
)

// The operator seed path for a ZDR test organization.
//
// The Frontend gateway refuses to turn org ZDR on while no AI provider is
// attested (409 zdr_no_attested_provider). org-core cannot read the model
// catalogue, so it carries no such gate, and a direct PATCH
// /api/v1/organizations/:id/settings is how an operator seeds ONE dedicated test
// org that has ZDR on while nothing is attested yet. The live proof of the ZDR
// work needs exactly that, so the seed path must keep working: a gate added to
// org-core, or a change to the delegation contract, would silently block it.
//
// The caller is apps/Frontend Plane/verevonv3/apps/gateway/scripts/zdr-seed-test-org.mjs,
// which signs the v3 delegation itself. These tests pin that signer to org-core's
// REAL verifier (serviceDelegationSignature, serviceAuthMiddleware,
// requireActiveMembership) with fixed vectors; the script's own test
// (zdr-seed-test-org.test.mjs) pins the same literals and fails if they drift from
// this file.
const (
	zdrSeedToken     = "zdr-seed-vector-credential-0123456789abcdef"
	zdrSeedTimestamp = "2026-10-06T12:00:00Z"
	zdrSeedOrgID     = "org-zdr-test-1"
	zdrSeedUserID    = "user-owner-1"
	zdrSeedUserRole  = "owner"

	zdrSeedPatchNonce     = "zdr-seed-nonce-0123456789ab-write"
	zdrSeedPatchURI       = "/api/v1/organizations/org-zdr-test-1/settings"
	zdrSeedPatchBody      = `{"zeroDataRetention":true}`
	zdrSeedPatchDigest    = "YpBp4jNws7-HykHuKUZzz3oy0yWeM68CprEfp8oYMTo"
	zdrSeedPatchSignature = "GgWAflcO4GXuGV2wgfPaYpvzjTxNzxGRlrFNdwmawZs"

	zdrSeedReadNonce     = "zdr-seed-nonce-0123456789ab-read"
	zdrSeedReadURI       = "/api/v1/organizations/org-zdr-test-1"
	zdrSeedReadDigest    = "47DEQpj8HBSa-_TImW-5JCeuQeRkm5NMpJWZG3hSuFU"
	zdrSeedReadSignature = "2Ys0-1OJrSMw6tHjVBU-KfYnUgrcGC4rkCRb6ag-jW8"
)

func zdrSeedClaims(method, uri, nonce, digest string) serviceDelegationClaims {
	return serviceDelegationClaims{
		Principal:  gatewayServicePrincipal,
		Audience:   serviceCredentialAudience,
		Timestamp:  zdrSeedTimestamp,
		Nonce:      nonce,
		Method:     method,
		URI:        uri,
		UserID:     zdrSeedUserID,
		OrgID:      zdrSeedOrgID,
		UserRole:   zdrSeedUserRole,
		BodySHA256: digest,
	}
}

func TestZdrSeedScriptSignerMatchesOrgCoreVerifier(t *testing.T) {
	if got := serviceDelegationBodyDigest([]byte(zdrSeedPatchBody)); got != zdrSeedPatchDigest {
		t.Fatalf("PATCH body digest=%s want=%s", got, zdrSeedPatchDigest)
	}
	if got := serviceDelegationBodyDigest(nil); got != zdrSeedReadDigest {
		t.Fatalf("empty body digest=%s want=%s", got, zdrSeedReadDigest)
	}
	patch := zdrSeedClaims(http.MethodPatch, zdrSeedPatchURI, zdrSeedPatchNonce, zdrSeedPatchDigest)
	if got := serviceDelegationSignature(zdrSeedToken, patch); got != zdrSeedPatchSignature {
		t.Fatalf("PATCH signature=%s want=%s", got, zdrSeedPatchSignature)
	}
	read := zdrSeedClaims(http.MethodGet, zdrSeedReadURI, zdrSeedReadNonce, zdrSeedReadDigest)
	if got := serviceDelegationSignature(zdrSeedToken, read); got != zdrSeedReadSignature {
		t.Fatalf("GET signature=%s want=%s", got, zdrSeedReadSignature)
	}
}

// zdrSeedMembers admits exactly the test org's owner: the membership guard
// re-checks the delegated user against org-core's own table before any
// org-scoped mutation.
type zdrSeedMembers struct{}

func (zdrSeedMembers) IsActiveMember(_ context.Context, orgID, userID string) (bool, error) {
	return orgID == zdrSeedOrgID && userID == zdrSeedUserID, nil
}

// zdrSeedRequest is what the script sends, built from the pinned literals: the
// headers are exactly the ones signedHeaders() in the script produces.
func zdrSeedRequest(method, uri, body, nonce, digest, signature string) *http.Request {
	request := httptest.NewRequest(method, uri, bytes.NewBufferString(body))
	request.Header.Set("X-Service-Id", gatewayServicePrincipal)
	request.Header.Set("X-Service-Token", zdrSeedToken)
	request.Header.Set("X-User-Id", zdrSeedUserID)
	request.Header.Set("X-Org-Id", zdrSeedOrgID)
	request.Header.Set("X-User-Role", zdrSeedUserRole)
	request.Header.Set("X-Delegation-Version", "v3")
	request.Header.Set("X-Delegation-Timestamp", zdrSeedTimestamp)
	request.Header.Set("X-Delegation-Nonce", nonce)
	request.Header.Set("X-Delegation-Body-SHA256", digest)
	request.Header.Set("X-Delegation-Signature", signature)
	if body != "" {
		request.Header.Set("Content-Type", "application/json")
	}
	return request
}

func zdrSeedPatchRequest() *http.Request {
	return zdrSeedRequest(http.MethodPatch, zdrSeedPatchURI, zdrSeedPatchBody, zdrSeedPatchNonce, zdrSeedPatchDigest, zdrSeedPatchSignature)
}

func zdrSeedReadRequest() *http.Request {
	return zdrSeedRequest(http.MethodGet, zdrSeedReadURI, "", zdrSeedReadNonce, zdrSeedReadDigest, zdrSeedReadSignature)
}

// zdrSeedRouter is the real service-auth middleware and the real membership
// guard in front of a probe that records who org-core would have acted for. The
// handler behind them in production, updateOrgSettings, needs a database; the
// route wiring behind these two is pinned by the next test.
func zdrSeedRouter(t *testing.T, now time.Time) *gin.Engine {
	t.Helper()
	credentials, err := parseServiceCredentials(`[{"principal":"verevon-gateway","audience":"org-core","token":"` + zdrSeedToken +
		`","scopes":["org:read:self","org:provision:self","org:settings:write:self","org:onboarding:write:self"]}]`)
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	probe := func(c *gin.Context) {
		c.JSON(http.StatusOK, gin.H{
			"user": c.GetHeader("X-User-Id"),
			"org":  c.GetHeader("X-Org-Id"),
			"role": c.GetHeader("X-User-Role"),
		})
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, func() time.Time { return now }))
	router.GET("/api/v1/organizations/:id", probe)
	router.PATCH("/api/v1/organizations/:id/settings", requireActiveMembership(zdrSeedMembers{}), probe)
	return router
}

func TestZdrSeedRequestIsAdmittedByTheRealMiddlewareAndMembershipGuard(t *testing.T) {
	now := time.Date(2026, 10, 6, 12, 0, 5, 0, time.UTC)
	router := zdrSeedRouter(t, now)

	for name, request := range map[string]*http.Request{
		"read the org first": zdrSeedReadRequest(),
		"switch ZDR":         zdrSeedPatchRequest(),
	} {
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusOK || !strings.Contains(response.Body.String(), `"user":"`+zdrSeedUserID+`"`) {
			t.Fatalf("%s: status=%d body=%s", name, response.Code, response.Body.String())
		}
	}
}

func TestZdrSeedRequestIsRefusedWhenAnyBoundFieldChanges(t *testing.T) {
	now := time.Date(2026, 10, 6, 12, 0, 5, 0, time.UTC)
	tampered := map[string]func(*http.Request){
		"another org":          func(r *http.Request) { r.Header.Set("X-Org-Id", "org-other") },
		"another role":         func(r *http.Request) { r.Header.Set("X-User-Role", "admin") },
		"body flipped to off":  func(r *http.Request) { r.Body = io.NopCloser(bytes.NewBufferString(`{"zeroDataRetention":false}`)) },
		"another timestamp":    func(r *http.Request) { r.Header.Set("X-Delegation-Timestamp", "2026-10-06T12:00:01Z") },
		"signature of the GET": func(r *http.Request) { r.Header.Set("X-Delegation-Signature", zdrSeedReadSignature) },
	}
	for name, tamper := range tampered {
		router := zdrSeedRouter(t, now)
		request := zdrSeedPatchRequest()
		tamper(request)
		response := httptest.NewRecorder()
		router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("%s: status=%d want=403 body=%s", name, response.Code, response.Body.String())
		}
	}

	// A signed request is single use, and goes stale after 30 seconds.
	router := zdrSeedRouter(t, now)
	for attempt, want := range []int{http.StatusOK, http.StatusForbidden} {
		response := httptest.NewRecorder()
		router.ServeHTTP(response, zdrSeedPatchRequest())
		if response.Code != want {
			t.Fatalf("attempt %d: status=%d want=%d", attempt+1, response.Code, want)
		}
	}
	stale := httptest.NewRecorder()
	zdrSeedRouter(t, now.Add(40*time.Second)).ServeHTTP(stale, zdrSeedPatchRequest())
	if stale.Code != http.StatusForbidden {
		t.Fatalf("stale request status=%d want=403", stale.Code)
	}
}

// The delegated user must be an active member of the org: a seed run names the
// test org's owner and nobody else's.
func TestZdrSeedRequestForANonMemberIsRefusedByTheGuard(t *testing.T) {
	now := time.Date(2026, 10, 6, 12, 0, 5, 0, time.UTC)
	credentials, err := parseServiceCredentials(`[{"principal":"verevon-gateway","audience":"org-core","token":"` + zdrSeedToken + `","scopes":["org:settings:write:self"]}]`)
	if err != nil {
		t.Fatalf("parse registry: %v", err)
	}
	router := gin.New()
	router.Use(serviceAuthMiddleware(credentials, func() time.Time { return now }))
	router.PATCH("/api/v1/organizations/:id/settings", requireActiveMembership(zdrSeedNoMembers{}), func(c *gin.Context) { c.Status(http.StatusOK) })

	response := httptest.NewRecorder()
	router.ServeHTTP(response, zdrSeedPatchRequest())
	if response.Code != http.StatusForbidden || !strings.Contains(response.Body.String(), "not a member") {
		t.Fatalf("status=%d body=%s", response.Code, response.Body.String())
	}
}

type zdrSeedNoMembers struct{}

func (zdrSeedNoMembers) IsActiveMember(context.Context, string, string) (bool, error) {
	return false, nil
}

// The route the seed uses is the very route the gateway proxies to: real
// router, real middleware. org-core stays ungated, so the seed keeps working
// while the gateway refuses.
func TestZdrSeedRouteIsTheSettingsRouteTheGatewayProxiesTo(t *testing.T) {
	t.Setenv(serviceCredentialEnv, `[{"principal":"verevon-gateway","audience":"org-core","token":"`+zdrSeedToken+
		`","scopes":["org:read:self","org:provision:self","org:settings:write:self","org:onboarding:write:self"]},{"principal":"auth-core","audience":"org-core","token":"zdr-seed-vector-auth-core-0123456789abcdef","scopes":["org:projection:write:any","org:projection:delete:any","org:retention:read:any"]}]`)
	server := NewServer(0, nil, nil, "", "")

	bound := ""
	for _, route := range server.router.Routes() {
		if route.Method == http.MethodPatch && route.Path == "/api/v1/organizations/:id/settings" {
			bound = route.Handler
		}
	}
	if !strings.Contains(bound, "updateOrgSettings") {
		t.Fatalf("PATCH /api/v1/organizations/:id/settings is bound to %q, want updateOrgSettings", bound)
	}

	// Without the gateway's credential the route is closed: the seed needs the
	// operator to hold it, and nothing else opens it.
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, httptest.NewRequest(http.MethodPatch, zdrSeedPatchURI, bytes.NewBufferString(zdrSeedPatchBody)))
	if response.Code != http.StatusUnauthorized {
		t.Fatalf("PATCH without a credential status=%d want=401", response.Code)
	}
	wrong := httptest.NewRequest(http.MethodPatch, zdrSeedPatchURI, bytes.NewBufferString(zdrSeedPatchBody))
	wrong.Header.Set("X-Service-Id", "auth-core")
	wrong.Header.Set("X-Service-Token", "zdr-seed-vector-auth-core-0123456789abcdef")
	denied := httptest.NewRecorder()
	server.router.ServeHTTP(denied, wrong)
	if denied.Code != http.StatusForbidden {
		t.Fatalf("PATCH as auth-core status=%d want=403 (only the gateway's credential carries org:settings:write:self)", denied.Code)
	}
}
