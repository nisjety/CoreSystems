package http

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"reflect"
	"sort"
	"strings"
	"testing"
	"time"

	orgcore "github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/org"
)

// Contract between audit-core's SIEM exporter (Go, WS06-S8) and org-core's REAL
// service-auth middleware, REAL route table and the REAL credential block from
// the Control Plane compose file, in the shape of the model entitlement
// contract (model_entitlement_contract_test.go).
//
// The golden under testdata/ is authored here, ahead of the exporter, and is the
// specification the exporter's client test replays: the requests it must send
// (path, query, headers) and the answers it must be able to read. Replaying it
// through org-core as deployed means a header, path or scope mismatch fails a
// test on this side before the exporter exists. It is pinned by a sha256 so a
// hand edit that the exporter's copy did not follow is caught.
//
// The compose block is the system under test, so it cannot be vendored. As with
// the model entitlement contract, a missing compose file skips unless
// REQUIRE_CONTRACT is set, and a compose file that lacks the audit-core
// principal FAILS: those are the lines WS06-S7 hands to the lead for
// docker-compose.yml, and CI must not pass without them.
const (
	siemContractGoldenPath = "testdata/audit-core-org-core-siem.contract.json"
	siemExporterScope      = "org:siem:read:any"
	siemManageScope        = "org:siem:manage:self"
)

type siemContractExchange struct {
	Name    string `json:"name"`
	Request struct {
		Method  string            `json:"method"`
		Path    string            `json:"path"`
		Headers map[string]string `json:"headers"`
	} `json:"request"`
	ExpectUpdatedSince string `json:"expectUpdatedSince"`
	Response           struct {
		Status       int            `json:"status"`
		CacheControl string         `json:"cacheControl"`
		Body         map[string]any `json:"body"`
	} `json:"response"`
}

type siemContractGolden struct {
	Contract         string                 `json:"contract"`
	Version          int                    `json:"version"`
	TokenPlaceholder string                 `json:"tokenPlaceholder"`
	Exchanges        []siemContractExchange `json:"exchanges"`
}

func siemGoldenBytes(t *testing.T) []byte {
	t.Helper()
	data, err := os.ReadFile(siemContractGoldenPath)
	if err != nil {
		t.Fatalf("read golden %s: %v", siemContractGoldenPath, err)
	}
	return data
}

func loadSIEMContractGolden(t *testing.T) siemContractGolden {
	t.Helper()
	var golden siemContractGolden
	decoder := json.NewDecoder(strings.NewReader(string(siemGoldenBytes(t))))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&golden); err != nil {
		t.Fatalf("decode golden: %v", err)
	}
	if golden.Version != 1 || golden.TokenPlaceholder != retentionTokenPlaceholder || len(golden.Exchanges) == 0 {
		t.Fatalf("golden is not a version 1 contract with exchanges: %+v", golden)
	}
	return golden
}

// TestSIEMContractGoldenMatchesItsPin fails when the golden was edited without
// re-pinning it, which would let the exporter's replay drift from org-core.
func TestSIEMContractGoldenMatchesItsPin(t *testing.T) {
	pin, err := os.ReadFile(siemContractGoldenPath + ".sha256")
	if err != nil {
		t.Fatalf("read the golden's sha256 pin: %v", err)
	}
	fields := strings.Fields(string(pin))
	if len(fields) == 0 {
		t.Fatalf("the golden's sha256 pin is empty")
	}
	if actual := sha256LF(siemGoldenBytes(t)); fields[0] != actual {
		t.Fatalf("golden differs from its pin: pinned %s, actual %s; re-pin it and update the exporter's copy together", fields[0], actual)
	}
}

func requireAuditCorePrincipal(t *testing.T, credentials []serviceCredential) serviceCredential {
	t.Helper()
	for _, credential := range credentials {
		if credential.Principal == siemExporterPrincipal {
			return credential
		}
	}
	t.Fatalf("the compose org-core registry has no %s principal; apply WS06-S7's docker-compose.yml lines (see the slice's lead_todo)", siemExporterPrincipal)
	return serviceCredential{}
}

func siemContractServer(t *testing.T, service siemDestinationService) (*Server, map[string]string) {
	t.Helper()
	raw, credentials, tokens := modelEntitlementComposeRegistry(t)
	requireAuditCorePrincipal(t, credentials)
	t.Setenv(serviceCredentialEnv, raw)
	t.Setenv(siemDestinationsFlagEnv, "1")
	server := NewServer(0, nil, nil, "", "")
	server.SetSIEMDestinations(service)
	return server, tokens
}

func TestComposeAuditCoreCredentialIsExactlyTheReviewedSIEMGrant(t *testing.T) {
	_, credentials, _ := modelEntitlementComposeRegistry(t)
	audit := requireAuditCorePrincipal(t, credentials)
	if !reflect.DeepEqual(audit.Scopes, []string{siemExporterScope}) {
		t.Fatalf("audit-core org-core scopes=%v want exactly [%s]", audit.Scopes, siemExporterScope)
	}

	holders := map[string][]string{}
	for _, credential := range credentials {
		for _, scope := range credential.Scopes {
			if scope == siemExporterScope || scope == siemManageScope {
				holders[scope] = append(holders[scope], credential.Principal)
			}
		}
	}
	for scope := range holders {
		sort.Strings(holders[scope])
	}
	if !reflect.DeepEqual(holders[siemExporterScope], []string{siemExporterPrincipal}) {
		t.Fatalf("holders of %s = %v; want exactly audit-core", siemExporterScope, holders[siemExporterScope])
	}
	if !reflect.DeepEqual(holders[siemManageScope], []string{gatewayServicePrincipal}) {
		t.Fatalf("holders of %s = %v; want exactly the gateway", siemManageScope, holders[siemManageScope])
	}
}

func TestComposeOrgCoreRegistryWithSIEMPassesStartupValidation(t *testing.T) {
	raw, credentials, _ := modelEntitlementComposeRegistry(t)
	requireAuditCorePrincipal(t, credentials)
	if err := ValidateRequiredServiceCredentialRegistry(raw); err != nil {
		t.Fatalf("org-core would refuse to start with the compose credential block: %v", err)
	}
}

func TestSIEMExporterRequestsAreAdmittedByRealOrgCoreMiddleware(t *testing.T) {
	golden := loadSIEMContractGolden(t)
	for _, exchange := range golden.Exchanges {
		t.Run(exchange.Name, func(t *testing.T) {
			wantBody, err := json.Marshal(exchange.Response.Body["data"])
			if err != nil {
				t.Fatalf("encode golden data: %v", err)
			}
			var canned []orgcore.SIEMExporterDestination
			if err := json.Unmarshal(wantBody, &canned); err != nil {
				t.Fatalf("golden data is not a destination list: %v", err)
			}
			fake := &fakeSIEM{exportRow: canned}
			server, tokens := siemContractServer(t, fake)

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
				t.Fatalf("org-core's real middleware and routes did not serve %s %s: status=%d want=%d body=%s", exchange.Request.Method, exchange.Request.Path, response.Code, exchange.Response.Status, response.Body.String())
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

			switch {
			case exchange.ExpectUpdatedSince == "" && fake.gotSince != nil:
				t.Fatalf("the route read updated_since=%v from a request that sent none", fake.gotSince)
			case exchange.ExpectUpdatedSince != "":
				want, err := time.Parse(time.RFC3339, exchange.ExpectUpdatedSince)
				if err != nil {
					t.Fatalf("golden updated_since: %v", err)
				}
				if fake.gotSince == nil || !fake.gotSince.Equal(want) {
					t.Fatalf("the route read updated_since=%v want %v", fake.gotSince, want)
				}
			}
			if fake.exported != 1 {
				t.Fatalf("the service was asked %d times, want 1", fake.exported)
			}
		})
	}
}

// The read returns plaintext secrets, so it is for audit-core alone: every other
// principal in the real compose registry is refused the same request, and the
// audit-core grant does not open any other route.
func TestOnlyAuditCoreMayReadSIEMDestinationsWithTheRealRegistry(t *testing.T) {
	fake := &fakeSIEM{}
	server, tokens := siemContractServer(t, fake)

	for principal, token := range tokens {
		if principal == siemExporterPrincipal {
			continue
		}
		request := httptest.NewRequest(http.MethodGet, "/internal/orgs/siem-destinations", nil)
		request.Header.Set("X-Service-Id", principal)
		request.Header.Set("X-Service-Token", token)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("principal %s reached the SIEM exporter read: status=%d body=%s", principal, response.Code, response.Body.String())
		}
	}
	if fake.exported != 0 {
		t.Fatalf("a refused principal reached the service %d times", fake.exported)
	}

	for _, path := range []string{
		"/api/v1/organizations/org-1",
		"/internal/orgs",
		"/internal/orgs/org-1",
		"/internal/orgs/org-1/retention-posture",
		"/internal/orgs/org-1/model-entitlement",
		"/orgs/org-1/siem-destinations",
	} {
		request := httptest.NewRequest(http.MethodGet, path, nil)
		request.Header.Set("X-Service-Id", siemExporterPrincipal)
		request.Header.Set("X-Service-Token", tokens[siemExporterPrincipal])
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusForbidden {
			t.Fatalf("audit-core read %s: status=%d want=%d (the grant must stay narrow)", path, response.Code, http.StatusForbidden)
		}
	}
}

func TestSIEMExporterRequestWithoutServiceHeadersIsRefused(t *testing.T) {
	golden := loadSIEMContractGolden(t)
	server, _ := siemContractServer(t, &fakeSIEM{})
	for _, exchange := range golden.Exchanges {
		request := httptest.NewRequest(exchange.Request.Method, exchange.Request.Path, nil)
		response := httptest.NewRecorder()
		server.router.ServeHTTP(response, request)
		if response.Code != http.StatusUnauthorized {
			t.Fatalf("%s %s without service headers status=%d want=%d", exchange.Request.Method, exchange.Request.Path, response.Code, http.StatusUnauthorized)
		}
	}
}

// With the flag off the exporter's request is answered 404, not served and not
// read as an organisation id, even with the real registry and the real grant.
func TestSIEMExporterRequestIs404WhileTheFlagIsOff(t *testing.T) {
	golden := loadSIEMContractGolden(t)
	raw, credentials, tokens := modelEntitlementComposeRegistry(t)
	requireAuditCorePrincipal(t, credentials)
	t.Setenv(serviceCredentialEnv, raw)
	t.Setenv(siemDestinationsFlagEnv, "")
	server := NewServer(0, nil, nil, "", "")
	fake := &fakeSIEM{}
	server.SetSIEMDestinations(fake)

	exchange := golden.Exchanges[0]
	request := httptest.NewRequest(exchange.Request.Method, exchange.Request.Path, nil)
	request.Header.Set("X-Service-Id", siemExporterPrincipal)
	request.Header.Set("X-Service-Token", tokens[siemExporterPrincipal])
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, request)
	if response.Code != http.StatusNotFound || fake.exported != 0 {
		t.Fatalf("flag off: status=%d body=%s exported=%d; want 404 and no service call", response.Code, response.Body.String(), fake.exported)
	}
}
