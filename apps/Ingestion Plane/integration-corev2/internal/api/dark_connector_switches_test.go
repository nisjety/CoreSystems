package api

import (
	"encoding/json"
	"errors"
	"net/http"
	"strings"
	"testing"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/actions"
	"github.com/triodelab/integration-corev2/internal/auth"
	"github.com/triodelab/integration-corev2/internal/customergateway"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Section 3.4 of the verification plan: CUSTOM_REST_CONNECTORS_ENABLED (W4-CC-6/7/8) and
// CUSTOMER_GATEWAY_CONNECTIONS_ENABLED (W4-BYO) leave the service exactly as it was while they are off: the
// routes they add answer the router's bare 404, and nothing is read or stored. The custom REST connector route
// is also checked with its switch on for the refusals its spec names (W4-CC-8).

func (r *cbRig) appWith(principal auth.Principal, customREST bool, gateway *customergateway.Broker, gatewayOn bool) *fiber.App {
	r.t.Helper()
	cfg, _, service := testOAuthStack(r.t)
	cfg.CredentialBindingsEnabled = r.enabled
	cfg.CustomRESTConnectorsEnabled = customREST
	cfg.CustomerGatewayConnectionsEnabled = gatewayOn
	cfg.CredentialRefSealKeys = "model-gateway=" + cbSealKey + ",execution-core=" + cbRedeemKey
	return NewServer(ServerConfig{
		Config: cfg, Repo: r.repo, OAuth: service, Events: &fakeEventsPublisher{}, OrgZDR: r.posture,
		Auth: fakeVerifier{principal: principal}, CredentialSealer: r.vault, CredentialHostVetter: r.vetter,
		CustomerGateway: gateway, Actions: actions.NewService(cfg, http.DefaultClient),
	})
}

func customRESTConnectorBodyJSON(name, bindingID string, capabilities []string) string {
	raw, _ := json.Marshal(map[string]any{"name": name, "credential_binding_id": bindingID, "capabilities": capabilities})
	return string(raw)
}

func (r *cbRig) connectionCount(org, provider string) int {
	r.t.Helper()
	list, err := r.mem.ListConnections(r.t.Context(), store.ConnectionFilter{OrganizationID: org, ProviderKey: provider})
	if err != nil {
		r.t.Fatal(err)
	}
	return len(list)
}

func TestTheCustomRESTConnectorRouteIsAbsentWhileItsSwitchIsOff(t *testing.T) {
	rig := newCBRig(t)
	bindingID := rig.createBinding(cbOrg, cbHost)
	callsBefore := rig.posture.calls
	app := rig.appWith(cbAdminPrincipal(cbOrg), false, nil, false)
	resp := cbSend(t, app, http.MethodPost, customRESTConnectorPath,
		customRESTConnectorBodyJSON("Billing API", bindingID, []string{actions.CustomRESTCapabilityRead}), bearerHeaders())
	if resp.status != http.StatusNotFound || resp.body["success"] != nil {
		t.Fatalf("custom REST connector with the switch off answered %d %s, want the router's bare 404", resp.status, resp.raw)
	}
	if rig.connectionCount(cbOrg, actions.CustomRESTProviderKey) != 0 || rig.posture.calls != callsBefore {
		t.Fatal("the dark custom REST route stored a connection or read the organisation's posture")
	}
}

func TestTheCustomRESTConnectorRouteNeedsTheBindingSwitchToo(t *testing.T) {
	rig := newCBRig(t)
	bindingID := rig.createBinding(cbOrg, cbHost)
	rig.enabled = false
	app := rig.appWith(cbAdminPrincipal(cbOrg), true, nil, false)
	resp := cbSend(t, app, http.MethodPost, customRESTConnectorPath,
		customRESTConnectorBodyJSON("Billing API", bindingID, nil), bearerHeaders())
	if resp.status != http.StatusNotFound || resp.body["success"] != nil {
		t.Fatalf("custom REST connector with bindings off answered %d %s, want the router's bare 404", resp.status, resp.raw)
	}
}

func TestACustomRESTConnectionCannotCallWhileItsSwitchIsOff(t *testing.T) {
	rig := newCBRig(t)
	bindingID := rig.createBinding(cbOrg, cbHost)
	if _, err := rig.mem.UpsertConnection(t.Context(), store.Connection{
		ID: "conn-rest-dark", ProviderKey: actions.CustomRESTProviderKey, ConnectorType: "custom_rest",
		OrganizationID: cbOrg, UserID: cbAdmin, Status: "active",
		ProviderContext: map[string]string{actions.CustomRESTBindingContextKey: bindingID},
		Capabilities:    []string{actions.CustomRESTCapabilityRead},
	}); err != nil {
		t.Fatal(err)
	}
	drainAudit(t, rig.mem)
	app := rig.appWith(cbAdminPrincipal(cbOrg), false, nil, false)
	body := `{"operation":"custom_rest.call","params":{"method":"GET","path":"/v1/items"}}`
	resp := cbSend(t, app, http.MethodPost, "/api/v1/connections/conn-rest-dark/actions", body, bearerHeaders())
	if resp.status == http.StatusOK || resp.body["success"] == true {
		t.Fatalf("a custom_rest call with the switch off answered %d %s, want a refusal", resp.status, resp.raw)
	}
	if rows := auditOfType(drainAudit(t, rig.mem), auditEventCredentialRedeemed); len(rows) != 0 {
		t.Fatalf("a dark custom_rest call opened the binding: %d redemption rows, want none", len(rows))
	}
}

func TestTheCustomRESTConnectorIsRefusedUnderZDRBeforeTheBodyIsRead(t *testing.T) {
	rig := newCBRig(t)
	bindingID := rig.createBinding(cbOrg, cbHost)
	for _, posture := range []fakeOrgPosture{{on: true}, {err: errors.New("org-core unavailable")}} {
		*rig.posture = posture
		app := rig.appWith(cbAdminPrincipal(cbOrg), true, nil, false)
		resp := cbSend(t, app, http.MethodPost, customRESTConnectorPath, `{"name":`, bearerHeaders())
		want := http.StatusConflict
		if posture.err != nil {
			want = http.StatusServiceUnavailable
		}
		if resp.status != want {
			t.Fatalf("posture %+v answered %d %s, want %d before the malformed body is read", posture, resp.status, resp.raw, want)
		}
		_ = bindingID
	}
	if rig.connectionCount(cbOrg, actions.CustomRESTProviderKey) != 0 {
		t.Fatal("a refused custom REST connector was stored")
	}
}

func TestOnlyAnOrgAdminCreatesACustomRESTConnectorThroughItsOwnRESTBinding(t *testing.T) {
	rig := newCBRig(t)
	bindingID := rig.createBinding(cbOrg, cbHost)
	otherBinding := rig.createBinding(cbOtherOrg, cbHost)

	member := auth.Principal{UserID: cbMember, OrganizationID: cbOrg, PrincipalType: "user", Role: "member"}
	if resp := cbSend(t, rig.appWith(member, true, nil, false), http.MethodPost, customRESTConnectorPath,
		customRESTConnectorBodyJSON("Billing API", bindingID, nil), bearerHeaders()); resp.status != http.StatusForbidden {
		t.Fatalf("a member answered %d %s, want 403", resp.status, resp.raw)
	}
	service := auth.Principal{UserID: "svc", OrganizationID: cbOrg, PrincipalType: "service", Role: "admin"}
	if resp := cbSend(t, rig.appWith(service, true, nil, false), http.MethodPost, customRESTConnectorPath,
		customRESTConnectorBodyJSON("Billing API", bindingID, nil), bearerHeaders()); resp.status != http.StatusUnauthorized && resp.status != http.StatusForbidden {
		t.Fatalf("a service principal answered %d %s, want a 401 or 403 refusal", resp.status, resp.raw)
	}

	admin := rig.appWith(cbAdminPrincipal(cbOrg), true, nil, false)
	if resp := cbSend(t, admin, http.MethodPost, customRESTConnectorPath,
		customRESTConnectorBodyJSON("Billing API", otherBinding, nil), bearerHeaders()); resp.status != http.StatusNotFound {
		t.Fatalf("another organisation's binding answered %d %s, want 404", resp.status, resp.raw)
	}
	if resp := cbSend(t, admin, http.MethodPost, customRESTConnectorPath,
		customRESTConnectorBodyJSON("Billing API", bindingID, []string{"gmail.send"}), bearerHeaders()); resp.status != http.StatusBadRequest {
		t.Fatalf("a foreign capability answered %d %s, want 400", resp.status, resp.raw)
	}
	if rig.connectionCount(cbOrg, actions.CustomRESTProviderKey) != 0 {
		t.Fatal("a refused request stored a connection")
	}

	resp := cbSend(t, admin, http.MethodPost, customRESTConnectorPath,
		customRESTConnectorBodyJSON("Billing API", bindingID, nil), bearerHeaders())
	if resp.status != http.StatusCreated {
		t.Fatalf("admin create answered %d %s, want 201", resp.status, resp.raw)
	}
	for _, leak := range []string{bindingID, cbHost, cbSecret} {
		if strings.Contains(resp.raw, leak) {
			t.Fatalf("the create answer echoes %q: %s", leak, resp.raw)
		}
	}
	data := dataOf(resp.body)
	if caps, _ := data["capabilities"].([]any); len(caps) != 0 {
		t.Fatalf("a connector created without capabilities was granted %v, want none", caps)
	}
	connectionID, _ := data["connection_id"].(string)
	stored, err := rig.mem.GetConnection(t.Context(), connectionID)
	if err != nil || stored.ProviderContext[actions.CustomRESTBindingContextKey] != bindingID || stored.OrganizationID != cbOrg {
		t.Fatalf("stored connection = %+v err=%v, want it bound to %s in %s", stored, err, bindingID, cbOrg)
	}
	if stored.EncryptedAccessToken != "" || stored.EncryptedRefreshToken != "" {
		t.Fatal("a custom REST connection stores a token; its credential is the binding")
	}
}

func TestCustomerGatewayRoutesAreAbsentWhileTheSwitchIsOff(t *testing.T) {
	rig := newCBRig(t)
	broker, err := customergateway.New(customergateway.Config{Vault: rig.vault, Client: http.DefaultClient})
	if err != nil {
		t.Fatal(err)
	}
	app := rig.appWith(cbAdminPrincipal(cbOrg), false, broker, false)
	createBody := `{"name":"Gateway","base_url":"https://gw.example.com/v1","api_key":"sk-gateway-0123456789abcdef","model_ids":["m-1"]}`
	for _, route := range []struct {
		method, path, body string
		headers            map[string]string
	}{
		{http.MethodPost, customerGatewayCreatePath, createBody, bearerHeaders()},
		{http.MethodGet, customerGatewayCreatePath, "", bearerHeaders()},
		{http.MethodDelete, customerGatewayCreatePath, `{"connection_id":"conn_x"}`, bearerHeaders()},
		{http.MethodPost, customerGatewayInferPath, `{"connection_id":"conn_x","org_id":"` + cbOrg + `","model":"m-1","messages":[]}`, keyHeaders("dev-key")},
		{http.MethodPost, customerGatewayStreamPath, `{"connection_id":"conn_x","org_id":"` + cbOrg + `","model":"m-1","messages":[]}`, keyHeaders("dev-key")},
	} {
		resp := cbSend(t, app, route.method, route.path, route.body, route.headers)
		if resp.status != http.StatusNotFound || resp.body["success"] != nil {
			t.Fatalf("%s %s with the switch off answered %d %s, want the router's bare 404", route.method, route.path, resp.status, resp.raw)
		}
	}
	if rig.connectionCount(cbOrg, customergateway.ProviderKey) != 0 {
		t.Fatal("a dark gateway route stored a connection")
	}
}
