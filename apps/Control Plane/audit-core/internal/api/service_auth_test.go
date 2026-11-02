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
	return `[{"principal":"verevon-gateway","audience":"audit-core","token":"` + testGatewayToken + `","scopes":["audit:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"` + testWriterToken + `","scopes":["audit:write"],"planes":["ingestion"]}]`
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
	if err := ValidateRequiredServiceCredentialRegistry(`[{"principal":"verevon-gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]}]`); err == nil {
		t.Fatal("expected missing integration-corev2 principal to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(`[{"principal":"verevon-gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"abcdef0123456789abcdef0123456789","scopes":["audit:write"],"planes":["ingestion","model"]}]`); err == nil {
		t.Fatal("expected integration-corev2 with authority outside ingestion to fail startup validation")
	}
	if err := ValidateRequiredServiceCredentialRegistry(`[{"principal":"gateway","audience":"audit-core","token":"0123456789abcdef0123456789abcdef","scopes":["audit:read:self"]},{"principal":"integration-corev2","audience":"audit-core","token":"abcdef0123456789abcdef0123456789","scopes":["audit:write"],"planes":["ingestion"]}]`); err == nil {
		t.Fatal("expected missing canonical verevon-gateway principal to fail startup validation")
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
	if authorization.Principal != "verevon-gateway" || authorization.Scope != "audit:read:self" || authorization.OrgID != "org-a" {
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
	request.Header.Set("X-Service-Id", "verevon-gateway")
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
		"v3", "verevon-gateway", "audit-core", timestampValue, nonce,
		request.Method, request.URL.RequestURI(), "user-a", orgID, "member", bodySHA,
	}, "\n")
	mac := hmac.New(sha256.New, []byte(testGatewayToken))
	_, _ = mac.Write([]byte(canonical))
	request.Header.Set("X-Delegation-Signature", base64.RawURLEncoding.EncodeToString(mac.Sum(nil)))
}

func TestComplianceScopeIsGrantedToTheGatewayPrincipalOnly(t *testing.T) {
	granted := `[{"principal":"verevon-gateway","audience":"audit-core","token":"` + testGatewayToken + `","scopes":["audit:read:self","audit:compliance:read:self"]}]`
	if _, err := parseServiceCredentialRegistry(granted); err != nil {
		t.Fatalf("gateway with the compliance scope rejected: %v", err)
	}
	for _, principal := range []string{"integration-corev2", "model-gateway", "verevon-gateway-2", "other-service"} {
		raw := `[{"principal":"` + principal + `","audience":"audit-core","token":"` + testGatewayToken + `","scopes":["audit:compliance:read:self"]}]`
		if _, err := parseServiceCredentialRegistry(raw); err == nil {
			t.Fatalf("principal %q was allowed to hold the compliance scope", principal)
		}
	}
}

func TestComplianceRoutesRequireTheComplianceScopeNotJustAuditRead(t *testing.T) {
	now := time.Now().UTC()
	readOnly, err := parseServiceCredentialRegistry(testCredentialRegistry())
	if err != nil {
		t.Fatal(err)
	}
	request := httptest.NewRequest(http.MethodGet, "/v1/compliance/audit/events?org_id=org-a", nil)
	signAuditReadRequest(request, now, "nonce-compliance-scope-1", "org-a")
	if _, failure := authorizeServiceRequest(request, readOnly, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("a gateway without audit:compliance:read:self reached the compliance API: %+v", failure)
	}

	withScope, err := parseServiceCredentialRegistry(complianceTestRegistry())
	if err != nil {
		t.Fatal(err)
	}
	request = httptest.NewRequest(http.MethodGet, "/v1/compliance/audit/events?org_id=org-a", nil)
	signAuditReadRequest(request, now, "nonce-compliance-scope-2", "org-a")
	authorization, failure := authorizeServiceRequest(request, withScope, newDelegationNonceCache(100), now)
	if failure != nil {
		t.Fatalf("compliance request rejected: %v", failure)
	}
	if authorization.Scope != "audit:compliance:read:self" || authorization.UserRole != "member" || authorization.OrgID != "org-a" {
		t.Fatalf("authorization = %+v; the role must come through for the handler's own check", authorization)
	}

	// A writer credential never reaches the compliance routes, and the
	// compliance scope does not unlock the older audit read route by itself.
	writer := httptest.NewRequest(http.MethodGet, "/v1/compliance/audit/events?org_id=org-a", nil)
	writer.Header.Set("X-Service-Id", "integration-corev2")
	writer.Header.Set("X-Service-Token", testWriterToken)
	if _, failure := authorizeServiceRequest(writer, withScope, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("writer on a compliance route: %+v", failure)
	}
	complianceOnly, err := parseServiceCredentialRegistry(`[{"principal":"verevon-gateway","audience":"audit-core","token":"` + testGatewayToken + `","scopes":["audit:compliance:read:self"]}]`)
	if err != nil {
		t.Fatal(err)
	}
	legacyRead := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(legacyRead, now, "nonce-compliance-scope-3", "org-a")
	if _, failure := authorizeServiceRequest(legacyRead, complianceOnly, newDelegationNonceCache(100), now); failure == nil || failure.Status != http.StatusForbidden {
		t.Fatalf("compliance scope unlocked GET /v1/audit: %+v", failure)
	}

	for method, path := range map[string]string{
		http.MethodGet:  "/v1/compliance/audit/unknown",
		http.MethodPost: "/v1/compliance/audit/events",
	} {
		if isComplianceRoute(method, path) {
			t.Fatalf("%s %s is not a compliance route", method, path)
		}
	}
}

// WS05-S1: the org role the read scope depends on comes only from the signed
// delegation. A forged role header without a valid signature never gets that far.
func TestForgedRoleHeaderWithUnsignedDelegationIsRefused(t *testing.T) {
	credentials, err := parseServiceCredentialRegistry(testCredentialRegistry())
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 7, 14, 12, 0, 0, 0, time.UTC)

	// No delegation headers at all, only a forged admin role.
	forged := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	forged.Header.Set("X-Service-Id", "verevon-gateway")
	forged.Header.Set("X-Service-Token", testGatewayToken)
	forged.Header.Set("X-User-Id", "user-a")
	forged.Header.Set("X-Org-Id", "org-a")
	forged.Header.Set("X-User-Role", "admin")
	_, failure := authorizeServiceRequest(forged, credentials, newDelegationNonceCache(100), now)
	if failure == nil || failure.Status != http.StatusForbidden || failure.Message != "signed service delegation required" {
		t.Fatalf("forged role without a delegation: %+v", failure)
	}

	// A member's signed delegation whose role header is swapped to admin after
	// signing no longer verifies, so the role cannot be escalated in transit.
	swapped := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(swapped, now, "nonce-role-swap-0123456", "org-a")
	swapped.Header.Set("X-User-Role", "admin")
	_, failure = authorizeServiceRequest(swapped, credentials, newDelegationNonceCache(100), now)
	if failure == nil || failure.Status != http.StatusForbidden || failure.Message != "signed service delegation required" {
		t.Fatalf("role swapped after signing: %+v", failure)
	}

	// The honestly signed member delegation reaches the handler as a non-admin.
	honest := httptest.NewRequest(http.MethodGet, "/v1/audit?org_id=org-a", nil)
	signAuditReadRequest(honest, now, "nonce-role-honest-01234", "org-a")
	authorization, failure := authorizeServiceRequest(honest, credentials, newDelegationNonceCache(100), now)
	if failure != nil || authorization.UserRole != "member" || authorization.IsOrgAdmin() {
		t.Fatalf("signed member delegation: %+v %+v", authorization, failure)
	}
}

func TestIsOrgAdminMatrix(t *testing.T) {
	for role, want := range map[string]bool{
		"owner": true, "admin": true, "Owner": true, "ADMIN": true, " admin ": true,
		"member": false, "viewer": false, "": false, "   ": false, "guest": false,
		"superadmin": false, "administrator": false, "owner,member": false, "admin;member": false,
	} {
		if got := (serviceAuthorization{UserRole: role}).IsOrgAdmin(); got != want {
			t.Fatalf("IsOrgAdmin(%q) = %t, want %t", role, got, want)
		}
	}
}
