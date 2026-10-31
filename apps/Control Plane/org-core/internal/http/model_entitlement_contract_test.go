package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"reflect"
	"sort"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/modelentitlement"
)

// Contract between inference-core's entitlement client (Rust, WS10-S3) and
// org-core's REAL service-auth middleware, REAL route table and the REAL
// credential block from the Control Plane compose file, in the shape of the
// retention posture contract (retention_posture_contract_test.go).
//
// The golden under testdata/ is authored here, ahead of the client, and is the
// specification the client's own test replays: the requests it must send (path,
// query, headers) and the answers it must be able to read. Replaying it through
// org-core as deployed means a header, path or scope mismatch fails a test on
// this side before the client exists.
//
// The compose block is the system under test, so it cannot be vendored. Like the
// retention contract, a missing compose file skips unless REQUIRE_CONTRACT is
// set. A compose file that lacks the inference-core principal FAILS: that is the
// lines WS10-S2 hands to the lead for docker-compose.yml, and CI must not pass
// without them.
const (
	modelEntitlementGoldenPath = "testdata/inference-core-org-core-model-entitlement.contract.json"
	// composeOverrideEnv points the compose-reading tests at another file. It is
	// for reviewing a proposed compose edit before it is applied; CI never sets it.
	composeOverrideEnv = "ORG_CORE_CONTRACT_COMPOSE_FILE"

	modelEntitlementScope = "org:model-entitlement:read:any"
)

type modelEntitlementExchange struct {
	Name    string `json:"name"`
	Request struct {
		Method  string            `json:"method"`
		Path    string            `json:"path"`
		Headers map[string]string `json:"headers"`
	} `json:"request"`
	Response struct {
		Status       int            `json:"status"`
		CacheControl string         `json:"cacheControl"`
		Body         map[string]any `json:"body"`
	} `json:"response"`
}

type modelEntitlementGolden struct {
	Contract         string                     `json:"contract"`
	Version          int                        `json:"version"`
	OrganizationID   string                     `json:"organizationId"`
	UserID           string                     `json:"userId"`
	TokenPlaceholder string                     `json:"tokenPlaceholder"`
	Exchanges        []modelEntitlementExchange `json:"exchanges"`
}

func loadModelEntitlementGolden(t *testing.T) modelEntitlementGolden {
	t.Helper()
	raw, err := os.ReadFile(modelEntitlementGoldenPath)
	if err != nil {
		t.Fatalf("read golden %s: %v", modelEntitlementGoldenPath, err)
	}
	var golden modelEntitlementGolden
	decoder := json.NewDecoder(strings.NewReader(string(raw)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if golden.Version != 1 || golden.TokenPlaceholder != retentionTokenPlaceholder || len(golden.Exchanges) == 0 {
		t.Fatalf("golden is not a version 1 contract with exchanges: %+v", golden)
	}
	return golden
}

// modelEntitlementComposeRegistry is composeOrgCoreRegistry with a path that a
// reviewer can redirect. It reads the same block the same way.
func modelEntitlementComposeRegistry(t *testing.T) (string, []serviceCredential, map[string]string) {
	t.Helper()
	path := controlComposePath
	if override := os.Getenv(composeOverrideEnv); override != "" {
		path = override
	}
	lines := strings.Split(strings.ReplaceAll(string(requireContractFile(t, path)), "\r\n", "\n"), "\n")
	found := -1
	for index, line := range lines {
		if strings.TrimSpace(line) == serviceCredentialEnv+": >-" {
			if found >= 0 {
				t.Fatalf("%s appears more than once in %s", serviceCredentialEnv, path)
			}
			found = index
		}
	}
	if found < 0 || found+1 >= len(lines) {
		t.Fatalf("%s block not found in %s", serviceCredentialEnv, path)
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
		credential.Token = "compose-contract-" + strings.ToLower(match[1]) + "-0123456789abcdef"
		tokens[credential.Principal] = credential.Token
		resolved = append(resolved, credential)
	}
	raw, err := json.Marshal(resolved)
	if err != nil {
		t.Fatalf("marshal registry: %v", err)
	}
	return string(raw), resolved, tokens
}

func modelEntitlementContractServer(t *testing.T, service modelEntitlementService) (*Server, map[string]string) {
	t.Helper()
	raw, credentials, tokens := modelEntitlementComposeRegistry(t)
	requireInferenceCorePrincipal(t, credentials)
	t.Setenv(serviceCredentialEnv, raw)
	server := NewServer(0, nil, nil, "", "")
	server.SetModelEntitlements(service)
	return server, tokens
}

func requireInferenceCorePrincipal(t *testing.T, credentials []serviceCredential) serviceCredential {
	t.Helper()
	for _, credential := range credentials {
		if credential.Principal == "inference-core" {
			return credential
		}
	}
	t.Fatalf("the compose org-core registry has no inference-core principal; apply WS10-S2's docker-compose.yml lines (see the slice's lead_todo)")
	return serviceCredential{}
}

func TestComposeInferenceCoreCredentialIsExactlyTheReviewedGrant(t *testing.T) {
	_, credentials, _ := modelEntitlementComposeRegistry(t)
	inference := requireInferenceCorePrincipal(t, credentials)
	if !reflect.DeepEqual(inference.Scopes, []string{modelEntitlementScope}) {
		t.Fatalf("inference-core org-core scopes=%v want exactly [%s]", inference.Scopes, modelEntitlementScope)
	}

	var holders []string
	for _, credential := range credentials {
		for _, scope := range credential.Scopes {
			if scope == modelEntitlementScope {
				holders = append(holders, credential.Principal)
			}
		}
	}
	sort.Strings(holders)
	if !reflect.DeepEqual(holders, []string{"inference-core", "model-gateway"}) {
		t.Fatalf("holders of %s = %v; want exactly inference-core and model-gateway", modelEntitlementScope, holders)
	}
}

func TestComposeOrgCoreRegistryWithModelEntitlementPassesStartupValidation(t *testing.T) {
	raw, credentials, _ := modelEntitlementComposeRegistry(t)
	requireInferenceCorePrincipal(t, credentials)
	if err := ValidateRequiredServiceCredentialRegistry(raw); err != nil {
		t.Fatalf("org-core would refuse to start with the compose credential block: %v", err)
	}
}

func TestInferenceCoreModelEntitlementRequestsAreAdmittedByRealOrgCoreMiddleware(t *testing.T) {
	golden := loadModelEntitlementGolden(t)
	for _, exchange := range golden.Exchanges {
		t.Run(exchange.Name, func(t *testing.T) {
			wantBody, err := json.Marshal(exchange.Response.Body)
			if err != nil {
				t.Fatalf("encode golden body: %v", err)
			}
			var canned modelentitlement.Resolution
			if err := json.Unmarshal(wantBody, &canned); err != nil {
				t.Fatalf("golden body is not a Resolution: %v", err)
			}
			resolver := &fakeResolver{resolution: canned}
			server, tokens := modelEntitlementContractServer(t, resolver)

			request := httptest.NewRequest(exchange.Request.Method, exchange.Request.Path, nil)
			for name, value := range exchange.Request.Headers {
				if value == golden.TokenPlaceholder {
					token, ok := tokens[exchange.Request.Headers["x-service-id"]]
					if !ok {
						t.Fatalf("golden presents unknown principal %q", exchange.Request.Headers["x-service-id"])
					}
					value = token
				}
				request.Header.Set(name, value)
			}
			response := httptest.NewRecorder()
			server.router.ServeHTTP(response, request)

			if response.Code != exchange.Response.Status {
				t.Fatalf("org-core's real middleware and routes did not serve the request %s %s: status=%d want=%d body=%s", exchange.Request.Method, exchange.Request.Path, response.Code, exchange.Response.Status, response.Body.String())
			}
			if got := response.Header().Get("Cache-Control"); got != exchange.Response.CacheControl {
				t.Fatalf("Cache-Control=%q want %q", got, exchange.Response.CacheControl)
			}
			var got map[string]any
			if err := json.Unmarshal(response.Body.Bytes(), &got); err != nil {
				t.Fatalf("decode response: %v", err)
			}
			if !reflect.DeepEqual(got, exchange.Response.Body) {
				t.Fatalf("response body differs from the golden\n got: %v\nwant: %v", got, exchange.Response.Body)
			}

			parsed, err := url.Parse(exchange.Request.Path)
			if err != nil {
				t.Fatalf("parse golden path: %v", err)
			}
			wantUser := parsed.Query().Get("user_id")
			if calls, org, user := resolver.snapshot(); calls != 1 || org != golden.OrganizationID || user != wantUser {
				t.Fatalf("resolver got calls=%d org=%q user=%q; want 1 call for org %q user %q", calls, org, user, golden.OrganizationID, wantUser)
			}
		})
	}
}

// The route is for inference-core and model-gateway alone: every other
// principal in the real compose registry is refused the same request, and the
// scope does not open the broad organisation reads.
func TestOnlyInferenceCoreAndModelGatewayMayReadModelEntitlement(t *testing.T) {
	resolver := &fakeResolver{}
	server, tokens := modelEntitlementContractServer(t, resolver)

	allowed := map[string]bool{"inference-core": true, "model-gateway": true}
	for principal, token := range tokens {
		if allowed[principal] {
			continue
		}
		request := httptest.NewRequest(http.MethodGet, "/internal/orgs/org-1/model-entitlement?user_id=u-1", nil)
		request.Header.Set("X-Service-Id", principal)
		request.Header.Set("X-Service-Token", token)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("principal %s reached the model entitlement route: status=%d body=%s", principal, response.Code, response.Body.String())
		}
	}
	if calls, _, _ := resolver.snapshot(); calls != 0 {
		t.Fatalf("a refused principal reached the resolver %d times", calls)
	}

	for _, path := range []string{
		"/api/v1/organizations/org-1",
		"/api/v1/organizations/org-1/entitlements",
		"/internal/orgs/org-1",
		"/internal/orgs",
		"/internal/orgs/org-1/retention-posture",
	} {
		request := httptest.NewRequest(http.MethodGet, path, nil)
		request.Header.Set("X-Service-Id", "inference-core")
		request.Header.Set("X-Service-Token", tokens["inference-core"])
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("inference-core read %s: status=%d want=%d (the grant must stay narrow)", path, response.Code, http.StatusForbidden)
		}
	}
}

func TestModelEntitlementRequestWithoutServiceHeadersIsRefused(t *testing.T) {
	golden := loadModelEntitlementGolden(t)
	server, _ := modelEntitlementContractServer(t, &fakeResolver{})
	for _, exchange := range golden.Exchanges {
		request := httptest.NewRequest(exchange.Request.Method, exchange.Request.Path, nil)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("%s %s without service headers status=%d want=%d", exchange.Request.Method, exchange.Request.Path, response.Code, http.StatusUnauthorized)
		}
	}
}
