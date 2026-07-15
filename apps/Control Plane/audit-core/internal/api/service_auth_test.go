package api

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"
)

const (
	testGatewayToken = "0123456789abcdef0123456789abcdef"
	testWriterToken  = "abcdef0123456789abcdef0123456789"
)

func testCredentialRegistry() string {
	return `[{"principal":"velion-gateway","audience":"audit-core","token":"` + testGatewayToken + `","scopes":["audit:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"` + testWriterToken + `","scopes":["audit:write"],"planes":["ingestion"]}]`
}

func TestParseServiceCredentialRegistryFailsClosed(t *testing.T) {
	valid, err := parseServiceCredentialRegistry(testCredentialRegistry())
	if err != nil || len(valid) != 2 {
		t.Fatalf("valid registry: credentials=%d err=%v", len(valid), err)
	}

	tests := []string{
		``,
		`not-json`,
		`[{"principal":"gateway","audience":"wrong","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]}]`,
		`[{"principal":"gateway","audience":"audit-core","token":"short","scopes":["audit:read:self"]}]`,
		`[{"principal":"gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:admin"]}]`,
		`[{"principal":"writer","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:write"]}]`,
		`[{"principal":"writer","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:write"],"planes":["ingestion"]},{"principal":"writer","audience":"audit-core","token":"abcdef0123456789abcdef0123456789","scopes":["audit:write"],"planes":["model"]}]`,
		`[{"principal":"gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]},{"principal":"writer","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:write"],"planes":["ingestion"]}]`,
	}
	for _, raw := range tests {
		if _, err := parseServiceCredentialRegistry(raw); err == nil {
			t.Fatalf("invalid registry was accepted: %s", raw)
		}
	}
}

func TestValidateRequiredServiceCredentialRegistry(t *testing.T) {
	if err := ValidateRequiredServiceCredentialRegistry(`[{"principal":"velion-gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]}]`); err == nil {
		t.Fatal("expected missing integration-corev2 principal to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(`[{"principal":"velion-gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"abcdef0123456789abcdef0123456789","scopes":["audit:write"],"planes":["ingestion","model"]}]`); err == nil {
		t.Fatal("expected integration-corev2 with authority outside ingestion to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(`[{"principal":"gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"abcdef0123456789abcdef0123456789","scopes":["audit:write"],"planes":["ingestion"]}]`); err == nil {
		t.Fatal("expected missing canonical velion-gateway principal to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(testCredentialRegistry()); err != nil {
		t.Fatalf("valid required registry rejected: %v", err)
	}
}

func TestAuthorizeAuditReadRequiresV3DelegationAndExactTenant(t *testing.T) {
	credentials, err := parseServiceCredentialRegistry(testCredentialRegistry())
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 7, 14, 12, 0, 0, 0, time.UTC)
	request := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a&limit=10", nil)
	signAuditReadRequest(request, now, "nonce-0123456789abcdef", "org-a")

	authorization, failure := authorizeServiceRequest(request, credentials, newDelegationNonceCache(100), now)
	if failure != nil {
		t.Fatalf("valid read delegation rejected: %v", failure)
	}
	if authorization.Principal != "velion-gateway" || authorization.Scope != "audit:read:self" || authorization.OrgID != "org-a" {
		t.Fatalf("authorization = %+v", authorization)
	}

	wrongOrg := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-b", nil)
	signAuditReadRequest(wrongOrg, now, "nonce-fedcba9876543210", "org-a")
	if _, failure := authorizeServiceRequest(wrongOrg, credentials, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("cross-tenant query failure = %+v; want 403", failure)
	}

	legacy := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	legacy.Header.Set("X-Internal-Api-Key", testGatewayToken)
	if _, failure := authorizeServiceRequest(legacy, credentials, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusUnauthorized {
		t.Fatalf("legacy key failure = %+v; want 401", failure)
	}
}

func TestAuthorizeAuditReadRejectsTamperingExpiryAndReplay(t *testing.T) {
	credentials, err := parseServiceCredentialRegistry(testCredentialRegistry())
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 7, 14, 12, 0, 0, 0, time.UTC)

	tampered := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(tampered, now, "nonce-0123456789abcdef", "org-a")
	tampered.URL.RawQuery = "org_id=org-a&limit=999"
	if _, failure := authorizeServiceRequest(tampered, credentials, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("tampered query failure = %+v; want 403", failure)
	}

	expired := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(expired, now.Add(-31*time.Second), "nonce-fedcba9876543210", "org-a")
	if _, failure := authorizeServiceRequest(expired, credentials, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("expired delegation failure = %+v; want 403", failure)
	}

	cache := newDelegationNonceCache(100)
	first := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(first, now, "nonce-replay-0123456789", "org-a")
	if _, failure := authorizeServiceRequest(first, credentials, cache, now); failure != nil {
		t.Fatalf("first nonce use rejected: %v", failure)
	}
	second := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(second, now, "nonce-replay-0123456789", "org-a")
	if _, failure := authorizeServiceRequest(second, credentials, cache, now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("replayed delegation failure = %+v; want 403", failure)
	}

	fractional := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequestAtTimestamp(fractional, "2026-07-14T12:00:00.123Z", "nonce-fractional-123456", "org-a")
	if _, failure := authorizeServiceRequest(fractional, credentials, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("fractional timestamp failure = %+v; want 403", failure)
	}

	duplicateClaim := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(duplicateClaim, now, "nonce-duplicate-1234567", "org-a")
	duplicateClaim.Header.Add("X-User-Id", "user-b")
	if _, failure := authorizeServiceRequest(duplicateClaim, credentials, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("duplicate delegated claim failure = %+v; want 403", failure)
	}
}

func TestAuthorizeAuditWriterPinsPrincipalPlanePolicy(t *testing.T) {
	credentials, err := parseServiceCredentialRegistry(testCredentialRegistry())
	if err != nil {
		t.Fatal(err)
	}
	request := httptest.NewRequest(http.MethodPost, "/v1/audit", strings.NewReader(`{"org_id":"org-a","plane":"ingestion","event":"sync"}`))
	request.Header.Set("X-Service-Id", "integration-corev2")
	request.Header.Set("X-Service-Token", testWriterToken)
	authorization, failure := authorizeServiceRequest(request, credentials, newDelegationNonceCache(100), time.Now())
	if failure != nil {
		t.Fatalf("writer rejected: %v", failure)
	}
	if authorization.Scope != "audit:write" || !authorization.AllowsPlane("ingestion") || authorization.AllowsPlane("model") {
		t.Fatalf("writer policy = %+v", authorization)
	}
}

func signAuditReadRequest(request *http.Request, timestamp time.Time, nonce, orgID string) {
	signAuditReadRequestAtTimestamp(request, timestamp.UTC().Format(time.RFC3339), nonce, orgID)
}

func signAuditReadRequestAtTimestamp(request *http.Request, timestampValue, nonce, orgID string) {
	request.Header.Set("X-Service-Id", "velion-gateway")
	request.Header.Set("X-Service-Token", testGatewayToken)
	request.Header.Set("X-User-Id", "user-a")
	request.Header.Set("X-Org-Id", orgID)
	request.Header.Set("X-User-Role", "member")
	request.Header.Set("X-Delegation-Version", "v3")
	request.Header.Set("X-Delegation-Timestamp", timestampValue)
	request.Header.Set("X-Delegation-Nonce", nonce)
	bodyDigest := sha256.Sum256(nil)
	bodySHA := base64.RawURLEncoding.EncodeToString(bodyDigest[:])
	request.Header.Set("X-Delegation-Body-SHA256", bodySHA)
	canonical := strings.Join([]string{
		"v3", "velion-gateway", "audit-core", timestampValue, nonce,
		request.Method, request.URL.RequestURI(), "user-a", orgID, "member", bodySHA,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(testGatewayToken))
	_, _ = mac.Write([]byte(canonical))
	request.Header.Set("X-Delegation-Signature", base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
}
