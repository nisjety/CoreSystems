package http

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

// These tests read the ORG_CORE_SERVICE_CREDENTIALS block that docker-compose.yml
// gives org-core. They fail until WS21-S3's docker-compose.yml line (the
// browser-broker principal, see the slice's lead_todo) is applied, and they must
// not pass without it: the broker cannot read a member's site decision otherwise.
const browserBrokerComposeToken = "compose-contract-browser_broker_org_core_service_token-0123456789abcdef"

// TestComposeGivesBrowserBrokerExactlyTheResolveScope pins the one principal that
// may read the site decision, and pins the variable its token is minted under.
func TestComposeGivesBrowserBrokerExactlyTheResolveScope(t *testing.T) {
	_, credentials, tokens := composeOrgCoreRegistry(t)

	var broker *serviceCredential
	for index := range credentials {
		credential := &credentials[index]
		for _, scope := range credential.Scopes {
			if scope == "org:browser-site:read:any" && credential.Principal != browserBrokerPrincipal {
				t.Fatalf("compose gives %s the browser-site read scope", credential.Principal)
			}
		}
		if credential.Principal == browserBrokerPrincipal {
			broker = credential
		}
	}
	if broker == nil {
		t.Fatalf("compose org-core registry has no %s principal; apply WS21-S3's docker-compose.yml line (see the slice's lead_todo)", browserBrokerPrincipal)
	}
	if len(broker.Scopes) != 1 || broker.Scopes[0] != "org:browser-site:read:any" {
		t.Fatalf("%s must hold exactly org:browser-site:read:any, got %v", browserBrokerPrincipal, broker.Scopes)
	}
	if tokens[browserBrokerPrincipal] != browserBrokerComposeToken {
		t.Fatalf("%s must present BROWSER_BROKER_ORG_CORE_SERVICE_TOKEN, got a token from another variable", browserBrokerPrincipal)
	}
}

// TestComposeBrokerResolveIsAdmittedByTheRealMiddleware proves the compose
// registry lets the broker past service authentication and scope checks on the
// resolve route. The handler is unconfigured here, so the answer is 503, not 401
// or 403.
func TestComposeBrokerResolveIsAdmittedByTheRealMiddleware(t *testing.T) {
	server, tokens := contractServer(t)
	request := httptest.NewRequest(http.MethodGet, browserSitesInternal+"?host=example.com", nil)
	request.Header.Set("X-Service-Id", browserBrokerPrincipal)
	request.Header.Set("X-Service-Token", tokens[browserBrokerPrincipal])
	response := httptest.NewRecorder()
	server.router.ServeHTTP(response, request)
	if response.Code != http.StatusServiceUnavailable {
		t.Fatalf("the compose broker must be admitted to the resolve route: status=%d body=%s", response.Code, response.Body.String())
	}
}
