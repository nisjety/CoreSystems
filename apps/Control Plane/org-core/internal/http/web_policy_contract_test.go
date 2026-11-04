package http

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"strings"
	"testing"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
)

// Cross-plane contract between Auth Core's web policy resolver (WS7.I, TypeScript)
// and org-core's REAL service-auth middleware, REAL route table and the REAL
// credential block from the Control Plane compose file, in the shape of the
// retention posture contract (retention_posture_contract_test.go).
//
// The golden is VENDORED into this module with a sha256 pin beside it. WS7.H writes
// it first, from the route's real answer; WS7.I's Auth Core spec
// (web-policy.org-core-contract.spec.ts, UPDATE_CONTRACT_GOLDEN=1) takes over as the
// writer and must reproduce these bytes or change both copies together. These tests
// replay it through org-core exactly as deployed, so a header, path, scope or answer
// mismatch on either side fails a test.
const webPolicyContractGoldenPath = "testdata/auth-core-org-core-web-policy.contract.json"

func webPolicyGoldenBytes(t *testing.T) []byte {
	t.Helper()
	data, err := os.ReadFile(webPolicyContractGoldenPath)
	if err != nil {
		t.Fatalf("read vendored golden %s: %v (regenerate with UPDATE_CONTRACT_GOLDEN=1 from auth-core)", webPolicyContractGoldenPath, err)
	}
	return data
}

// TestWebPolicyContractCopyMatchesPin fails when the vendored golden was edited
// without regenerating it from Auth Core's real resolver.
func TestWebPolicyContractCopyMatchesPin(t *testing.T) {
	pin, err := os.ReadFile(webPolicyContractGoldenPath + ".sha256")
	if err != nil {
		t.Fatalf("read the golden's sha256 pin: %v", err)
	}
	fields := strings.Fields(string(pin))
	if len(fields) == 0 {
		t.Fatal("the golden's sha256 pin is empty")
	}
	if actual := sha256LF(webPolicyGoldenBytes(t)); fields[0] != actual {
		t.Fatalf("vendored golden differs from its pin: pinned %s, actual %s; regenerate it with UPDATE_CONTRACT_GOLDEN=1 in auth-core, never by hand", fields[0], actual)
	}
}

func loadWebPolicyContractGolden(t *testing.T) retentionContractGolden {
	t.Helper()
	var golden retentionContractGolden
	decoder := json.NewDecoder(strings.NewReader(string(webPolicyGoldenBytes(t))))
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

// webPolicyGoldenView is the service answer the golden's response describes, built
// from the golden itself so the replay checks the route's serialisation and not a
// second copy of the values.
func webPolicyGoldenView(t *testing.T, golden retentionContractGolden) *orgcore.WebPolicyView {
	t.Helper()
	data, _ := golden.Response.Body["data"].(map[string]any)
	policyJSON, err := json.Marshal(data["policy"])
	if err != nil {
		t.Fatalf("encode golden policy: %v", err)
	}
	var policy orgcore.WebPolicyDoc
	if err := json.Unmarshal(policyJSON, &policy); err != nil {
		t.Fatalf("decode golden policy: %v", err)
	}
	entitlements := map[string]bool{}
	for key, value := range data["entitlements"].(map[string]any) {
		entitlements[key] = value == true
	}
	return &orgcore.WebPolicyView{
		Exists: true, Policy: policy, Effective: policy,
		Revision: int64(data["revision"].(float64)), Rev: data["rev"].(string), Entitlements: entitlements,
	}
}

// goldenWebPolicyService answers the golden's view and nothing else.
type goldenWebPolicyService struct {
	view  *orgcore.WebPolicyView
	err   error
	calls int
}

func (g *goldenWebPolicyService) GetWebPolicyPosture(context.Context, string) (*orgcore.WebPolicyView, error) {
	g.calls++
	return g.view, g.err
}

func (g *goldenWebPolicyService) SetWebPolicy(context.Context, string, string, string, orgcore.WebPolicyDoc, int64) (*orgcore.WebPolicyPutResult, error) {
	return nil, errors.New("not used")
}

func (g *goldenWebPolicyService) ListWebPolicyHistory(context.Context, string, int, int64) ([]orgcore.WebPolicyHistoryEntry, int64, error) {
	return nil, 0, errors.New("not used")
}

func assertJSONEqualsGolden(t *testing.T, response *httptest.ResponseRecorder, want retentionContractAnswer) {
	t.Helper()
	var body map[string]any
	if err := json.Unmarshal(response.Body.Bytes(), &body); err != nil {
		t.Fatalf("decode response %q: %v", response.Body.String(), err)
	}
	if !reflect.DeepEqual(body, want.Body) {
		t.Fatalf("response=%v golden=%v", body, want.Body)
	}
}

func TestAuthCoreWebPolicyRequestIsAdmittedByRealOrgCoreMiddleware(t *testing.T) {
	golden := loadWebPolicyContractGolden(t)
	server, tokens := contractServer(t)
	service := &goldenWebPolicyService{view: webPolicyGoldenView(t, golden)}
	server.webPolicy = service
	for index, recorded := range golden.Requests {
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, goldenHTTPRequest(t, golden, index, tokens))
		if response.Code != golden.Response.Status {
			t.Fatalf("org-core's real middleware and routes did not serve Auth Core's request %s %s: status=%d want=%d body=%s", recorded.Method, recorded.Path, response.Code, golden.Response.Status, response.Body.String())
		}
		assertJSONEqualsGolden(t, response, golden.Response)
		if response.Header().Get("Cache-Control") != "no-store" {
			t.Fatalf("Cache-Control=%q, want no-store", response.Header().Get("Cache-Control"))
		}
	}
	if service.calls != len(golden.Requests) {
		t.Fatalf("service reads=%d want %d", service.calls, len(golden.Requests))
	}
}

// Auth Core treats exactly this typed 404 as "org-core has no record of the org"
// and every other failure as unknown, so the real handler must keep producing it.
func TestWebPolicyNotFoundAnswerMatchesGolden(t *testing.T) {
	golden := loadWebPolicyContractGolden(t)
	server, tokens := contractServer(t)
	server.webPolicy = &goldenWebPolicyService{err: orgcore.ErrNotFound}

	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, goldenHTTPRequest(t, golden, 0, tokens))
	if response.Code != golden.NotFound.Status {
		t.Fatalf("status=%d want=%d body=%s", response.Code, golden.NotFound.Status, response.Body.String())
	}
	assertJSONEqualsGolden(t, response, golden.NotFound)
}

func TestWebPolicyRequestWithoutServiceHeadersIsRefused(t *testing.T) {
	golden := loadWebPolicyContractGolden(t)
	server, _ := contractServer(t)
	server.webPolicy = &goldenWebPolicyService{view: webPolicyGoldenView(t, golden)}
	for _, recorded := range golden.Requests {
		request := httptest.NewRequest(recorded.Method, recorded.Path, nil)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("%s %s without service headers status=%d want=%d", recorded.Method, recorded.Path, response.Code, http.StatusUnauthorized)
		}
	}
}

// TestOnlyAuthCoreMayReadWebPolicyThroughTheComposeRegistry replays the golden
// request with every OTHER principal of the real compose registry: all are refused
// before the service is reached, and auth-core's own credential does not open the
// broad organization reads.
func TestOnlyAuthCoreMayReadWebPolicyThroughTheComposeRegistry(t *testing.T) {
	golden := loadWebPolicyContractGolden(t)
	server, tokens := contractServer(t)
	service := &goldenWebPolicyService{view: webPolicyGoldenView(t, golden)}
	server.webPolicy = service

	for principal, token := range tokens {
		if principal == "auth-core" {
			continue
		}
		request := httptest.NewRequest(http.MethodGet, golden.Requests[0].Path, nil)
		request.Header.Set("X-Service-Id", principal)
		request.Header.Set("X-Service-Token", token)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("principal %s reached the web policy route: status=%d body=%s", principal, response.Code, response.Body.String())
		}
	}
	if service.calls != 0 {
		t.Fatalf("a principal other than auth-core reached the service %d times", service.calls)
	}
	if tokens["auth-core"] == "" {
		t.Fatal("auth-core is missing from the compose org-core registry")
	}
	for _, path := range []string{
		"/internal/orgs/org-contract-fixture",
		"/internal/orgs",
		"/orgs/org-contract-fixture/web-policy",
		"/api/v1/organizations/org-contract-fixture/web-policy",
	} {
		request := httptest.NewRequest(http.MethodGet, path, nil)
		request.Header.Set("X-Service-Id", "auth-core")
		request.Header.Set("X-Service-Token", tokens["auth-core"])
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("auth-core read %s: status=%d want=%d (the grant must stay narrow)", path, response.Code, http.StatusForbidden)
		}
	}
}
