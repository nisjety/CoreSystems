package http

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"regexp"
	"sort"
	"strings"
	"testing"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
)

// Cross-language contract between Auth Core's retention resolver (TypeScript)
// and org-core's REAL service-auth middleware, REAL route table and the REAL
// credential block from the Control Plane compose file.
//
// Auth Core used to look an org's ZDR posture up with no service headers. The
// real middleware answers 401 to that, Auth Core treated every 401 as an
// outage and resolved "not ZDR" for every org, and no test noticed because the
// resolver's own spec mocked fetch and enforced no authentication. The golden
// below is written by the real TypeScript resolver
// (auth-core/src/auth/interactive-retention-policy.org-core-contract.spec.ts);
// these tests replay it through org-core exactly as deployed, so a header, path
// or scope mismatch on either side fails a test.
//
// The golden is VENDORED into this module (testdata/, with a sha256 pin beside
// it) so the replay runs from this module's own Docker build context and reads
// nothing outside it. Auth Core's spec writes the canonical copy and compares it
// with this one, and the repo-level drift check pins both; this test fails when
// the copy no longer matches its pin. Being inside the module, a missing copy is
// a failure, never a skip.
//
// Only the Control Plane compose file sits outside the module: it is the system
// under test (the REAL credential block), so it cannot be vendored. Without
// REQUIRE_CONTRACT a missing compose file skips the test; with it a missing file
// fails, so a CI job cannot pass by not finding the contract.
const (
	retentionContractGoldenPath = "testdata/auth-core-org-core-retention-posture.contract.json"
	controlComposePath          = "../../../docker-compose.yml"
	retentionTokenPlaceholder   = "<ORG_CORE_SERVICE_TOKEN>"
)

type retentionContractRequest struct {
	Method  string            `json:"method"`
	Path    string            `json:"path"`
	Headers map[string]string `json:"headers"`
}

type retentionContractGolden struct {
	Contract         string                     `json:"contract"`
	Version          int                        `json:"version"`
	OrganizationID   string                     `json:"organizationId"`
	TokenPlaceholder string                     `json:"tokenPlaceholder"`
	Requests         []retentionContractRequest `json:"requests"`
	Response         retentionContractAnswer    `json:"response"`
	NotFound         retentionContractAnswer    `json:"notFound"`
}

type retentionContractAnswer struct {
	Status int            `json:"status"`
	Body   map[string]any `json:"body"`
}

func requireContractFile(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err == nil {
		return data
	}
	if os.IsNotExist(err) && os.Getenv("REQUIRE_CONTRACT") == "" {
		t.Skipf("%s is absent (set REQUIRE_CONTRACT=1 to make that a failure)", path)
	}
	t.Fatalf("read contract file %s: %v", path, err)
	return nil
}

// retentionGoldenBytes reads the vendored golden. It sits inside this module, so
// its absence is always a failure.
func retentionGoldenBytes(t *testing.T) []byte {
	t.Helper()
	data, err := os.ReadFile(retentionContractGoldenPath)
	if err != nil {
		t.Fatalf("read vendored golden %s: %v (regenerate with UPDATE_CONTRACT_GOLDEN=1 from auth-core)", retentionContractGoldenPath, err)
	}
	return data
}

// sha256LF is the digest both sides pin: the file with CRLF normalised to LF.
func sha256LF(data []byte) string {
	sum := sha256.Sum256([]byte(strings.ReplaceAll(string(data), "\r\n", "\n")))
	return hex.EncodeToString(sum[:])
}

// TestRetentionContractGoldenMatchesItsPin fails when the vendored golden was
// edited without regenerating it from Auth Core's real resolver.
func TestRetentionContractGoldenMatchesItsPin(t *testing.T) {
	pin, err := os.ReadFile(retentionContractGoldenPath + ".sha256")
	if err != nil {
		t.Fatalf("read the golden's sha256 pin: %v", err)
	}
	fields := strings.Fields(string(pin))
	if len(fields) == 0 {
		t.Fatalf("the golden's sha256 pin is empty")
	}
	if actual := sha256LF(retentionGoldenBytes(t)); fields[0] != actual {
		t.Fatalf("vendored golden differs from its pin: pinned %s, actual %s; regenerate it with UPDATE_CONTRACT_GOLDEN=1 in auth-core, never by hand", fields[0], actual)
	}
}

func loadRetentionContractGolden(t *testing.T) retentionContractGolden {
	t.Helper()
	var golden retentionContractGolden
	decoder := json.NewDecoder(strings.NewReader(string(retentionGoldenBytes(t))))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if golden.Version != 1 || golden.TokenPlaceholder != retentionTokenPlaceholder || len(golden.Requests) == 0 ||
		golden.Response.Status == 0 || golden.NotFound.Status == 0 {
		t.Fatalf("golden is not a version 1 contract with requests: %+v", golden)
	}
	return golden
}

var composeVariablePattern = regexp.MustCompile(`\$\{([A-Za-z0-9_]+)(?::\?[^}]*)?\}`)

// composeOrgCoreRegistry reads the ORG_CORE_SERVICE_CREDENTIALS block that
// docker-compose.yml really gives org-core and substitutes a distinct,
// startup-valid token for every ${VARIABLE}. It returns the registry as the
// env value org-core would receive, the parsed credentials and the token by
// principal.
func composeOrgCoreRegistry(t *testing.T) (string, []serviceCredential, map[string]string) {
	t.Helper()
	lines := strings.Split(strings.ReplaceAll(string(requireContractFile(t, controlComposePath)), "\r\n", "\n"), "\n")
	found := -1
	for index, line := range lines {
		if strings.TrimSpace(line) == serviceCredentialEnv+": >-" {
			if found >= 0 {
				t.Fatalf("%s appears more than once in %s", serviceCredentialEnv, controlComposePath)
			}
			found = index
		}
	}
	if found < 0 || found+1 >= len(lines) {
		t.Fatalf("%s block not found in %s", serviceCredentialEnv, controlComposePath)
	}

	var declared []serviceCredential
	if err := json.Unmarshal([]byte(strings.TrimSpace(lines[found+1])), &declared); err != nil {
		t.Fatalf("decode compose %s: %v", serviceCredentialEnv, err)
	}
	tokens := make(map[string]string, len(declared))
	resolved := make([]serviceCredential, 0, len(declared))
	for _, credential := range declared {
		match := composeVariablePattern.FindStringSubmatch(credential.Token)
		if match == nil || match[0] != credential.Token {
			t.Fatalf("principal %s token is not a single ${VARIABLE} reference: %q", credential.Principal, credential.Token)
		}
		credential.Token = fmt.Sprintf("compose-contract-%s-0123456789abcdef", strings.ToLower(match[1]))
		tokens[credential.Principal] = credential.Token
		resolved = append(resolved, credential)
	}
	raw, err := json.Marshal(resolved)
	if err != nil {
		t.Fatalf("marshal registry: %v", err)
	}
	return string(raw), resolved, tokens
}

// contractServer builds org-core's real HTTP server (real router, real
// middleware, real routes) with the compose registry installed.
func contractServer(t *testing.T) (*Server, map[string]string) {
	t.Helper()
	raw, _, tokens := composeOrgCoreRegistry(t)
	t.Setenv(serviceCredentialEnv, raw)
	return NewServer(0, nil, nil, "", ""), tokens
}

func goldenHTTPRequest(t *testing.T, golden retentionContractGolden, index int, tokens map[string]string) *http.Request {
	t.Helper()
	recorded := golden.Requests[index]
	request := httptest.NewRequest(recorded.Method, recorded.Path, nil)
	for name, value := range recorded.Headers {
		if value == retentionTokenPlaceholder {
			token, ok := tokens[recorded.Headers["x-service-id"]]
			if !ok {
				t.Fatalf("golden presents unknown principal %q", recorded.Headers["x-service-id"])
			}
			value = token
		}
		request.Header.Set(name, value)
	}
	return request
}

func TestComposeAuthCoreCredentialScopesAreExactlyTheReviewedSet(t *testing.T) {
	_, credentials, _ := composeOrgCoreRegistry(t)
	for _, credential := range credentials {
		if credential.Principal != "auth-core" {
			continue
		}
		got := append([]string(nil), credential.Scopes...)
		sort.Strings(got)
		// org:domain:read:any (WS05-S6) is the by-domain read auth-core makes for the SSO enforcement hook.
		want := []string{"org:domain:read:any", "org:projection:delete:any", "org:projection:write:any", "org:retention:read:any", "org:web-policy:read:any"}
		if strings.Join(got, ",") != strings.Join(want, ",") {
			t.Fatalf("auth-core org-core scopes=%v want exactly %v", got, want)
		}
		return
	}
	t.Fatal("auth-core principal missing from the compose org-core registry")
}

func TestComposeOrgCoreRegistryPassesStartupValidation(t *testing.T) {
	raw, _, _ := composeOrgCoreRegistry(t)
	if err := ValidateRequiredServiceCredentialRegistry(raw); err != nil {
		t.Fatalf("org-core would refuse to start with the compose credential block: %v", err)
	}
}

func TestAuthCoreRetentionPostureRequestIsAdmittedByRealOrgCoreMiddleware(t *testing.T) {
	golden := loadRetentionContractGolden(t)
	server, tokens := contractServer(t)
	server.retentionPosture = &fakeRetentionPostureReader{posture: &orgcore.RetentionPosture{
		ZDRIntent: golden.Response.Body["zdrIntent"] == true,
		Entitled:  golden.Response.Body["entitled"] == true,
	}}
	for index, recorded := range golden.Requests {
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, goldenHTTPRequest(t, golden, index, tokens))
		if response.Code != golden.Response.Status {
			t.Fatalf("org-core's real middleware and routes did not serve Auth Core's request %s %s: status=%d want=%d body=%s", recorded.Method, recorded.Path, response.Code, golden.Response.Status, response.Body.String())
		}
		assertBodyEqualsGolden(t, response, golden.Response)
	}
}

// The resolver treats exactly this 404 body as "org-core has no record of the
// org" (not ZDR, not remembered) and every other 404 as a failure, so the real
// handler must keep producing it.
func TestRetentionPostureNotFoundAnswerMatchesGolden(t *testing.T) {
	golden := loadRetentionContractGolden(t)
	server, tokens := contractServer(t)
	server.retentionPosture = &fakeRetentionPostureReader{err: orgcore.ErrNotFound}

	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, goldenHTTPRequest(t, golden, 0, tokens))
	if response.Code != golden.NotFound.Status {
		t.Fatalf("status=%d want=%d body=%s", response.Code, golden.NotFound.Status, response.Body.String())
	}
	assertBodyEqualsGolden(t, response, golden.NotFound)
}

func assertBodyEqualsGolden(t *testing.T, response *httptest.ResponseRecorder, want retentionContractAnswer) {
	t.Helper()
	var body map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode response: %v", err)
	}
	if len(body) != len(want.Body) {
		t.Fatalf("response=%v golden=%v", body, want.Body)
	}
	for name, wantValue := range want.Body {
		if got, ok := body[name]; !ok || got != wantValue {
			t.Fatalf("response field %s=%v want=%v (response=%v)", name, got, wantValue, body)
		}
	}
}

// The scope has exactly TWO holders: Auth Core (its token issuer) and
// quarry-edge (the fire-time schedule preflight, R-13 PR 2, which broke the old
// one-holder invariant on purpose). Every other principal in the real compose
// registry is refused the same request, and neither holder's credential opens
// the broad organization reads it must not (WS15.S4a reads two booleans).
var retentionPostureHolders = map[string]bool{"auth-core": true, "quarry-edge": true}

func TestOnlyAuthCoreAndQuarryEdgeMayReadRetentionPosture(t *testing.T) {
	server, tokens := contractServer(t)
	reader := &fakeRetentionPostureReader{posture: &orgcore.RetentionPosture{ZDRIntent: true, Entitled: true}}
	server.retentionPosture = reader

	for principal, token := range tokens {
		if retentionPostureHolders[principal] {
			continue
		}
		request := httptest.NewRequest(http.MethodGet, "/internal/orgs/org-1/retention-posture", nil)
		request.Header.Set("X-Service-Id", principal)
		request.Header.Set("X-Service-Token", token)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("principal %s reached the posture route: status=%d body=%s", principal, response.Code, response.Body.String())
		}
	}
	if _, calls := reader.snapshot(); calls != 0 {
		t.Fatalf("a principal outside the two holders reached the reader %d times", calls)
	}

	for holder := range retentionPostureHolders {
		if tokens[holder] == "" {
			t.Fatalf("holder %s is missing from the compose org-core registry", holder)
		}
		request := httptest.NewRequest(http.MethodGet, "/internal/orgs/org-1/retention-posture", nil)
		request.Header.Set("X-Service-Id", holder)
		request.Header.Set("X-Service-Token", tokens[holder])
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusOK {
			t.Fatalf("holder %s was refused the posture route: status=%d body=%s", holder, response.Code, response.Body.String())
		}

		for _, path := range []string{
			"/api/v1/organizations/org-1",
			"/api/v1/organizations/org-1/entitlements",
			"/internal/orgs/org-1",
			"/internal/orgs",
		} {
			request := httptest.NewRequest(http.MethodGet, path, nil)
			request.Header.Set("X-Service-Id", holder)
			request.Header.Set("X-Service-Token", tokens[holder])
			response := httptest.NewRecorder()
			server.router.ServeHTTP(response, request)
			if response.Code != http.StatusForbidden {
				t.Fatalf("%s read %s: status=%d want=%d (the grant must stay narrow)", holder, path, response.Code, http.StatusForbidden)
			}
		}
	}
	if _, calls := reader.snapshot(); calls != len(retentionPostureHolders) {
		t.Fatalf("reader called %d times, want once per holder (%d)", calls, len(retentionPostureHolders))
	}
}

// TestComposeRetentionPostureScopeHasExactlyTwoHolders is the registry-level
// form of the rule: it reads the scope lists, so a third holder fails even if the
// route table later changed.
func TestComposeRetentionPostureScopeHasExactlyTwoHolders(t *testing.T) {
	_, credentials, _ := composeOrgCoreRegistry(t)
	var holders []string
	for _, credential := range credentials {
		for _, scope := range credential.Scopes {
			if scope == "org:retention:read:any" {
				holders = append(holders, credential.Principal)
			}
		}
	}
	sort.Strings(holders)
	if strings.Join(holders, ",") != "auth-core,quarry-edge" {
		t.Fatalf("org:retention:read:any holders=%v want exactly [auth-core quarry-edge]", holders)
	}
}

// quarryEdgeComposeToken is the token the compose file gives quarry-edge, derived
// the way composeOrgCoreRegistry derives every token (one distinct value per
// variable), so it also pins the variable the token is minted under.
const quarryEdgeComposeToken = "compose-contract-quarry_org_core_service_token-0123456789abcdef"

// TestQuarryEdgePrincipalHoldsOnlyRetentionRead pins what Quarry may do inside
// org-core. Of every org-core read it holds the retention-posture read and
// nothing broader (no org:read:*, no tenant, projection, siem, browser-site or
// agent/model policy scope). Its only other scopes are the two key-custody scopes
// (WS17.E2), which route to nothing until ORG_CORE_KEYS_ENABLED=1 and which no
// other principal holds. Any new scope for quarry-edge must change this list.
func TestQuarryEdgePrincipalHoldsOnlyRetentionRead(t *testing.T) {
	_, credentials, tokens := composeOrgCoreRegistry(t)
	var quarry *serviceCredential
	for index := range credentials {
		if credentials[index].Principal == "quarry-edge" {
			quarry = &credentials[index]
		}
	}
	if quarry == nil {
		t.Fatal("quarry-edge principal missing from the compose org-core registry (R-13 PR 2)")
	}
	got := append([]string(nil), quarry.Scopes...)
	sort.Strings(got)
	want := []string{"org:keys:unwrap:any", "org:keys:wrap:any", "org:retention:read:any"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("quarry-edge org-core scopes=%v want exactly %v", got, want)
	}
	reads := 0
	for _, scope := range got {
		if strings.HasSuffix(scope, ":read:any") || strings.HasSuffix(scope, ":read") {
			reads++
			if scope != "org:retention:read:any" {
				t.Fatalf("quarry-edge holds a read scope beyond the retention posture: %s", scope)
			}
		}
		if strings.HasSuffix(scope, ":self") {
			t.Fatalf("quarry-edge holds a delegated :self scope: %s", scope)
		}
	}
	if reads != 1 {
		t.Fatalf("quarry-edge holds %d read scopes, want exactly the retention posture read", reads)
	}
	if quarry.Audience != serviceCredentialAudience {
		t.Fatalf("quarry-edge audience=%q", quarry.Audience)
	}
	if tokens["quarry-edge"] != quarryEdgeComposeToken {
		t.Fatalf("quarry-edge must present QUARRY_ORG_CORE_SERVICE_TOKEN (the secrets-ledger name), got a token from another variable: %q", tokens["quarry-edge"])
	}
}

// TestComposeKeyAndWebPolicyScopeHolders: the key-custody scopes are
// quarry-edge's alone and the web-policy read is Auth Core's alone, so a
// stray grant of either fails here before it reaches a stack.
func TestComposeKeyAndWebPolicyScopeHolders(t *testing.T) {
	_, credentials, _ := composeOrgCoreRegistry(t)
	holders := map[string][]string{}
	for _, credential := range credentials {
		for _, scope := range credential.Scopes {
			if strings.HasPrefix(scope, "org:keys:") || strings.HasPrefix(scope, "org:web-policy:") {
				holders[scope] = append(holders[scope], credential.Principal)
			}
		}
	}
	want := map[string]string{
		"org:keys:wrap:any":       "quarry-edge",
		"org:keys:unwrap:any":     "quarry-edge",
		"org:web-policy:read:any": "auth-core",
	}
	if len(holders) != len(want) {
		t.Fatalf("scopes under org:keys:* and org:web-policy:* held in compose=%v want exactly %v", holders, want)
	}
	for scope, principal := range want {
		if len(holders[scope]) != 1 || holders[scope][0] != principal {
			t.Fatalf("%s holders=%v want only %s", scope, holders[scope], principal)
		}
	}
}

func TestRetentionPostureRequestWithoutServiceHeadersIsRefused(t *testing.T) {
	golden := loadRetentionContractGolden(t)
	server, _ := contractServer(t)
	for _, recorded := range golden.Requests {
		request := httptest.NewRequest(recorded.Method, recorded.Path, nil)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("%s %s without service headers status=%d want=%d", recorded.Method, recorded.Path, response.Code, http.StatusUnauthorized)
		}
	}
}
