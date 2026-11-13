package api

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"strings"
	"testing"
	"time"

	"github.com/gofiber/fiber/v2"

	"github.com/triodelab/integration-corev2/internal/auth"
	secretcrypto "github.com/triodelab/integration-corev2/internal/crypto"
	"github.com/triodelab/integration-corev2/internal/egress"
	"github.com/triodelab/integration-corev2/internal/store"
)

// Credential bindings (W4-CC-3) and credential references (W4-CC-4), through the real routes, the real
// vault and the memory store. Dark behind CREDENTIAL_BINDINGS_ENABLED.

const (
	cbOrg       = "org-cb"
	cbOtherOrg  = "org-cb-other"
	cbAdmin     = "user-cb-admin"
	cbMember    = "user-cb-member"
	cbSecret    = "fixture-bearer-value-zzzzzzzzzzzzzzzzzz"
	cbHost      = "api.example.com"
	cbSealKey   = "mg-seal-key-aaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	cbRedeemKey = "ec-redeem-key-bbbbbbbbbbbbbbbbbbbbbbbbbb"
	cbKeyHeader = "X-Internal-API-Key"
)

// cbVetter stands in for the egress guard: it records the hosts it was asked about and answers err.
type cbVetter struct {
	err   error
	hosts []string
}

func (v *cbVetter) ResolveAndVet(_ context.Context, host string) ([]netip.Addr, error) {
	v.hosts = append(v.hosts, host)
	if v.err != nil {
		return nil, v.err
	}
	return []netip.Addr{netip.MustParseAddr("93.184.216.34")}, nil
}

type cbRig struct {
	t       *testing.T
	mem     *store.MemoryRepository
	vault   *secretcrypto.Vault
	vetter  *cbVetter
	posture *fakeOrgPosture
	key     []byte
	enabled bool
	repo    store.Repository
}

func newCBRig(t *testing.T) *cbRig {
	t.Helper()
	cfg, mem, _ := testOAuthStack(t)
	vault, err := secretcrypto.NewVault(cfg.EncryptionKey)
	if err != nil {
		t.Fatal(err)
	}
	return &cbRig{t: t, mem: mem, vault: vault, vetter: &cbVetter{}, posture: &fakeOrgPosture{}, key: cfg.EncryptionKey, enabled: true, repo: mem}
}

func (r *cbRig) app(principal auth.Principal) *fiber.App {
	r.t.Helper()
	cfg, _, service := testOAuthStack(r.t)
	cfg.CredentialBindingsEnabled = r.enabled
	cfg.CredentialRefSealKeys = "model-gateway=" + cbSealKey + ",execution-core=" + cbRedeemKey
	return NewServer(ServerConfig{
		Config: cfg, Repo: r.repo, OAuth: service, Events: &fakeEventsPublisher{}, OrgZDR: r.posture,
		Auth: fakeVerifier{principal: principal}, CredentialSealer: r.vault, CredentialHostVetter: r.vetter,
	})
}

type cbResponse struct {
	status int
	header http.Header
	raw    string
	body   map[string]any
}

func cbSend(t *testing.T, app *fiber.App, method, target, body string, headers map[string]string) cbResponse {
	t.Helper()
	req := httptest.NewRequest(method, target, strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	for key, value := range headers {
		req.Header.Set(key, value)
	}
	resp, err := app.Test(req)
	if err != nil {
		t.Fatalf("app.Test %s %s: %v", method, target, err)
	}
	defer resp.Body.Close()
	raw, _ := io.ReadAll(resp.Body)
	var decoded map[string]any
	_ = json.Unmarshal(raw, &decoded)
	return cbResponse{status: resp.StatusCode, header: resp.Header, raw: string(raw), body: decoded}
}

func bearerHeaders() map[string]string { return map[string]string{"Authorization": "Bearer token-1"} }

func keyHeaders(key string) map[string]string { return map[string]string{cbKeyHeader: key} }

func cbAdminPrincipal(org string) auth.Principal {
	return auth.Principal{UserID: cbAdmin, OrganizationID: org, PrincipalType: "user", Role: "admin"}
}

func cbCreateBody(kind, host, header, secret string) string {
	raw, _ := json.Marshal(map[string]any{
		"kind": kind, "host": host, "header_name": header, "allowed_methods": []string{"GET", "post"},
		"allowed_path_prefixes": []string{"/v1/"}, "secret": secret,
	})
	return string(raw)
}

func reasonCode(body map[string]any) string {
	errObj, _ := body["error"].(map[string]any)
	details, _ := errObj["details"].(map[string]any)
	reason, _ := details["reason_code"].(string)
	return reason
}

// createBinding creates a binding through the route as an admin of org and returns its id.
func (r *cbRig) createBinding(org, host string) string {
	r.t.Helper()
	resp := cbSend(r.t, r.app(cbAdminPrincipal(org)), http.MethodPost, credentialBindingsPath, cbCreateBody("rest_bearer", host, "Authorization", cbSecret), bearerHeaders())
	if resp.status != http.StatusCreated {
		r.t.Fatalf("create binding: %d %s", resp.status, resp.raw)
	}
	binding, _ := dataOf(resp.body)["binding"].(map[string]any)
	id, _ := binding["id"].(string)
	if !strings.HasPrefix(id, credentialBindingIDPrefix) {
		r.t.Fatalf("created binding id %q, want the cb_ prefix", id)
	}
	return id
}

func (r *cbRig) bindingCount(org string) int {
	r.t.Helper()
	list, err := r.mem.ListCredentialBindings(r.t.Context(), org)
	if err != nil {
		r.t.Fatal(err)
	}
	return len(list)
}

func TestCredentialBindingRoutesAreAbsentWhileTheSwitchIsOff(t *testing.T) {
	rig := newCBRig(t)
	rig.enabled = false
	app := rig.app(cbAdminPrincipal(cbOrg))
	for _, route := range []struct {
		method, path string
		headers      map[string]string
	}{
		{http.MethodPost, credentialBindingsPath, bearerHeaders()},
		{http.MethodGet, credentialBindingsPath, bearerHeaders()},
		{http.MethodDelete, credentialBindingsPath + "/cb_x", bearerHeaders()},
		{http.MethodPost, credentialRefSealPath, keyHeaders(cbSealKey)},
		{http.MethodPost, credentialRefRedeemPath, keyHeaders(cbRedeemKey)},
	} {
		resp := cbSend(t, app, route.method, route.path, cbCreateBody("rest_bearer", cbHost, "Authorization", cbSecret), route.headers)
		if resp.status != http.StatusNotFound || resp.body["success"] != nil {
			t.Fatalf("%s %s with the switch off answered %d %s, want the router's bare 404", route.method, route.path, resp.status, resp.raw)
		}
	}
	if rig.bindingCount(cbOrg) != 0 || len(rig.vetter.hosts) != 0 || rig.posture.calls != 0 {
		t.Fatal("a dark route stored a binding, resolved a host or read the organisation's posture")
	}
}

func TestAnAdminCreatesABindingWhoseSecretIsSealedToItsIdAndNeverReturned(t *testing.T) {
	rig := newCBRig(t)
	app := rig.app(cbAdminPrincipal(cbOrg))
	resp := cbSend(t, app, http.MethodPost, credentialBindingsPath, cbCreateBody("rest_api_key", "API.Example.com:443", "x-api-key", cbSecret), bearerHeaders())
	if resp.status != http.StatusCreated {
		t.Fatalf("create: %d %s", resp.status, resp.raw)
	}
	if strings.Contains(resp.raw, cbSecret) || strings.Contains(strings.ToLower(resp.raw), "ciphertext") {
		t.Fatalf("the create answer carries the secret or its ciphertext: %s", resp.raw)
	}
	binding, _ := dataOf(resp.body)["binding"].(map[string]any)
	id, _ := binding["id"].(string)
	if binding["host"] != cbHost || binding["headerName"] != "X-Api-Key" || binding["status"] != store.CredentialBindingStatusActive {
		t.Fatalf("created binding %v, want the normalised host, the canonical header and status active", binding)
	}
	if len(rig.vetter.hosts) != 1 || rig.vetter.hosts[0] != cbHost {
		t.Fatalf("the host vetted was %v, want exactly the normalised host", rig.vetter.hosts)
	}

	stored, err := rig.mem.GetCredentialBindingForRedeem(t.Context(), cbOrg, id)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(stored.SecretCiphertext), cbSecret) {
		t.Fatal("the stored ciphertext holds the secret in clear")
	}
	if plain, err := rig.vault.Decrypt(string(stored.SecretCiphertext), []byte(id)); err != nil || plain != cbSecret {
		t.Fatalf("the stored ciphertext does not open to the secret under its own binding id: %v", err)
	}
	if _, err := rig.vault.Decrypt(string(stored.SecretCiphertext), []byte(credentialBindingIDPrefix+"another")); err == nil {
		t.Fatal("the ciphertext opens under another binding id; it is not bound to its own")
	}
	if stored.KeyVersion != credentialBindingKeyVersion || stored.CreatedByUserID != cbAdmin || stored.OrganizationID != cbOrg {
		t.Fatalf("stored binding %v, want key version v1, the admin as creator and the admin's organisation", stored)
	}

	list := cbSend(t, app, http.MethodGet, credentialBindingsPath, "", bearerHeaders())
	if list.status != http.StatusOK || strings.Contains(list.raw, cbSecret) || strings.Contains(list.raw, string(stored.SecretCiphertext)) {
		t.Fatalf("the list answered %d and carries the secret or its ciphertext: %s", list.status, list.raw)
	}
	items, _ := dataOf(list.body)["bindings"].([]any)
	if len(items) != 1 {
		t.Fatalf("list returned %d bindings, want 1", len(items))
	}

	created := auditOfType(drainAudit(t, rig.mem), auditEventCredentialBindingCreated)
	if len(created) != 1 || len(created[0].Metadata) != 1 || created[0].Metadata["bindingId"] != id || created[0].OrganizationID != cbOrg {
		t.Fatalf("created audit rows %+v, want one row carrying only the binding id", created)
	}
	if dump := fmt.Sprintf("%+v", created[0]); strings.Contains(dump, cbHost) || strings.Contains(dump, cbSecret) {
		t.Fatal("the created audit row names the host; it carries ids only")
	}
}

func TestOnlyAnOrgAdminUserMayManageBindings(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	cases := []struct {
		name      string
		principal auth.Principal
		headers   map[string]string
		status    int
	}{
		{"a member", auth.Principal{UserID: cbMember, OrganizationID: cbOrg, PrincipalType: "user", Role: "member"}, bearerHeaders(), http.StatusForbidden},
		{"a service principal with an admin role", auth.Principal{UserID: "svc", OrganizationID: cbOrg, PrincipalType: "service", Role: "admin", Scopes: []string{"integration:write", "integration:read"}}, bearerHeaders(), http.StatusUnauthorized},
		{"the shared internal key", cbAdminPrincipal(cbOrg), keyHeaders("dev-key"), http.StatusUnauthorized},
	}
	for _, c := range cases {
		app := rig.app(c.principal)
		for _, route := range []struct{ method, path string }{
			{http.MethodPost, credentialBindingsPath},
			{http.MethodGet, credentialBindingsPath},
			{http.MethodDelete, credentialBindingsPath + "/" + id},
		} {
			resp := cbSend(t, app, route.method, route.path, cbCreateBody("rest_bearer", cbHost, "Authorization", cbSecret), c.headers)
			if resp.status != c.status {
				t.Fatalf("%s: %s %s answered %d %s, want %d", c.name, route.method, route.path, resp.status, resp.raw, c.status)
			}
		}
	}
	if rig.bindingCount(cbOrg) != 1 {
		t.Fatal("a refused caller changed the organisation's bindings")
	}
	if stored, _ := rig.mem.GetCredentialBinding(t.Context(), cbOrg, id); stored.Status != store.CredentialBindingStatusActive {
		t.Fatal("a refused caller revoked the binding")
	}
}

func TestABindingIsRefusedForAZDROrganisationAndWhenThePostureIsUnknown(t *testing.T) {
	for _, c := range []struct {
		name   string
		on     bool
		err    error
		status int
		code   string
	}{
		{"ZDR on", true, nil, http.StatusConflict, "credential_binding_zdr_refused"},
		{"posture unknown", false, errors.New("org-core unavailable"), http.StatusServiceUnavailable, "zdr_org_state_unavailable"},
	} {
		t.Run(c.name, func(t *testing.T) {
			rig := newCBRig(t)
			rig.posture.on, rig.posture.err = c.on, c.err
			resp := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodPost, credentialBindingsPath, cbCreateBody("rest_bearer", cbHost, "Authorization", cbSecret), bearerHeaders())
			if resp.status != c.status || errorCode(resp.body) != c.code {
				t.Fatalf("create answered %d %s, want %d %s", resp.status, resp.raw, c.status, c.code)
			}
			if strings.Contains(resp.raw, cbSecret) {
				t.Fatal("the refusal echoes the secret")
			}
			if rig.bindingCount(cbOrg) != 0 || len(rig.vetter.hosts) != 0 || len(drainAudit(t, rig.mem)) != 0 {
				t.Fatal("a refused binding was stored, resolved or audited")
			}
		})
	}
}

func TestTheBindingHostMustBeAPublicDNSNameOnPort443(t *testing.T) {
	for _, host := range []string{
		"10.0.0.1", "1.2.3.4", "127.1", "[::1]", "::1", "*.example.com", "user@example.com", "user:pw@example.com",
		"example.com:8443", "example.com:80", "localhost", "example.com.", "example.com/path", "https://example.com",
		"exa mple.com", "-example.com", "example-.com", "",
	} {
		rig := newCBRig(t)
		resp := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodPost, credentialBindingsPath, cbCreateBody("rest_bearer", host, "Authorization", cbSecret), bearerHeaders())
		if resp.status != http.StatusBadRequest || errorCode(resp.body) != "invalid_host" {
			t.Errorf("host %q answered %d %s, want 400 invalid_host", host, resp.status, resp.raw)
		}
		if len(rig.vetter.hosts) != 0 || rig.bindingCount(cbOrg) != 0 {
			t.Errorf("host %q was resolved or stored before the grammar refused it", host)
		}
	}

	for _, c := range []struct {
		err  error
		code string
	}{
		{fmt.Errorf("%w: private address", egress.ErrBlocked), "host_not_public"},
		{errors.New("no such host"), "host_unresolvable"},
	} {
		rig := newCBRig(t)
		rig.vetter.err = c.err
		resp := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodPost, credentialBindingsPath, cbCreateBody("rest_bearer", "internal.example.com", "Authorization", cbSecret), bearerHeaders())
		if resp.status != http.StatusBadRequest || errorCode(resp.body) != c.code {
			t.Errorf("vetter error %v answered %d %s, want 400 %s", c.err, resp.status, resp.raw, c.code)
		}
		if rig.bindingCount(cbOrg) != 0 {
			t.Error("a binding to a host that is not public was stored")
		}
	}
}

func TestTheBindingShapeIsAClosedList(t *testing.T) {
	body := func(mutate func(map[string]any)) string {
		fields := map[string]any{
			"kind": "rest_bearer", "host": cbHost, "header_name": "Authorization", "allowed_methods": []string{"GET"},
			"allowed_path_prefixes": []string{"/v1/"}, "secret": cbSecret,
		}
		mutate(fields)
		raw, _ := json.Marshal(fields)
		return string(raw)
	}
	cases := map[string]struct {
		mutate func(map[string]any)
		code   string
	}{
		"an unknown kind":                     {func(f map[string]any) { f["kind"] = "oauth" }, "invalid_kind"},
		"a bearer kind outside Authorization": {func(f map[string]any) { f["header_name"] = "X-Api-Key" }, "invalid_header_name"},
		"a header not on the list":            {func(f map[string]any) { f["kind"] = "rest_api_key"; f["header_name"] = "Cookie" }, "invalid_header_name"},
		"a method not on the list":            {func(f map[string]any) { f["allowed_methods"] = []string{"TRACE"} }, "invalid_allowed_methods"},
		"no methods":                          {func(f map[string]any) { f["allowed_methods"] = []string{} }, "invalid_allowed_methods"},
		"a relative prefix":                   {func(f map[string]any) { f["allowed_path_prefixes"] = []string{"v1/"} }, "invalid_allowed_path_prefixes"},
		"a dot segment":                       {func(f map[string]any) { f["allowed_path_prefixes"] = []string{"/v1/../admin"} }, "invalid_allowed_path_prefixes"},
		"an encoded segment":                  {func(f map[string]any) { f["allowed_path_prefixes"] = []string{"/v1/%2e%2e/"} }, "invalid_allowed_path_prefixes"},
		"a query":                             {func(f map[string]any) { f["allowed_path_prefixes"] = []string{"/v1/?x=1"} }, "invalid_allowed_path_prefixes"},
		"an empty secret":                     {func(f map[string]any) { f["secret"] = "" }, "invalid_secret"},
		"a secret with a space":               {func(f map[string]any) { f["secret"] = "Bearer abc" }, "invalid_secret"},
		"a secret with a newline":             {func(f map[string]any) { f["secret"] = "abc\r\nX-Injected: 1" }, "invalid_secret"},
	}
	for name, c := range cases {
		rig := newCBRig(t)
		resp := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodPost, credentialBindingsPath, body(c.mutate), bearerHeaders())
		if resp.status != http.StatusBadRequest || errorCode(resp.body) != c.code {
			t.Errorf("%s answered %d %s, want 400 %s", name, resp.status, resp.raw, c.code)
		}
		if strings.Contains(resp.raw, cbSecret) || rig.bindingCount(cbOrg) != 0 {
			t.Errorf("%s: the refusal echoed the secret or a binding was stored", name)
		}
	}
}

func TestRevokingABindingEmptiesItsSecretOnceAndIsAllowedUnderZDR(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	drainAudit(t, rig.mem)

	// Another organisation's admin cannot see or revoke it.
	if resp := cbSend(t, rig.app(cbAdminPrincipal(cbOtherOrg)), http.MethodDelete, credentialBindingsPath+"/"+id, "", bearerHeaders()); resp.status != http.StatusNotFound {
		t.Fatalf("another organisation's revoke answered %d %s, want 404", resp.status, resp.raw)
	}

	rig.posture.on = true
	resp := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodDelete, credentialBindingsPath+"/"+id, "", bearerHeaders())
	if resp.status != http.StatusOK {
		t.Fatalf("revoke under ZDR answered %d %s, want 200: removing a standing credential is never refused", resp.status, resp.raw)
	}
	stored, err := rig.mem.GetCredentialBindingForRedeem(t.Context(), cbOrg, id)
	if err != nil {
		t.Fatal(err)
	}
	if stored.Status != store.CredentialBindingStatusRevoked || stored.RevokedAt == nil || len(stored.SecretCiphertext) != 0 {
		t.Fatalf("revoked binding %v (ciphertext %d bytes), want revoked, stamped and emptied", stored, len(stored.SecretCiphertext))
	}
	if again := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodDelete, credentialBindingsPath+"/"+id, "", bearerHeaders()); again.status != http.StatusNotFound {
		t.Fatalf("a second revoke answered %d, want 404", again.status)
	}
	revoked := auditOfType(drainAudit(t, rig.mem), auditEventCredentialBindingRevoked)
	if len(revoked) != 1 || len(revoked[0].Metadata) != 1 || revoked[0].Metadata["bindingId"] != id {
		t.Fatalf("revoked audit rows %+v, want one row carrying only the binding id", revoked)
	}
}

// --- credential references (W4-CC-4) ---

func cbSealBody(bindingID, org, host string, deadline time.Time, zdr, waived *bool) string {
	fields := map[string]any{
		"binding_id": bindingID, "org_id": org, "connection_id": "conn-1", "call_id": "call-1", "slot": "api_key",
		"host": host, "deadline": deadline.UTC().Format(time.RFC3339Nano),
	}
	if zdr != nil {
		fields["zdr"] = *zdr
	}
	if waived != nil {
		fields["waived"] = *waived
	}
	raw, _ := json.Marshal(fields)
	return string(raw)
}

func boolRef(v bool) *bool { return &v }

// seal asks for a reference as the model-gateway and returns the response.
func (r *cbRig) seal(body string) cbResponse {
	r.t.Helper()
	return cbSend(r.t, r.app(auth.Principal{}), http.MethodPost, credentialRefSealPath, body, keyHeaders(cbSealKey))
}

func (r *cbRig) redeem(ref, host, callID string) cbResponse {
	r.t.Helper()
	raw, _ := json.Marshal(map[string]string{"ref": ref, "host": host, "call_id": callID})
	return cbSend(r.t, r.app(auth.Principal{}), http.MethodPost, credentialRefRedeemPath, string(raw), keyHeaders(cbRedeemKey))
}

func (r *cbRig) sealedRef(bindingID string) string {
	r.t.Helper()
	resp := r.seal(cbSealBody(bindingID, cbOrg, cbHost, time.Now().Add(time.Minute), boolRef(false), boolRef(false)))
	if resp.status != http.StatusOK {
		r.t.Fatalf("seal: %d %s", resp.status, resp.raw)
	}
	ref, _ := dataOf(resp.body)["ref"].(string)
	if !strings.HasPrefix(ref, "crf1.") {
		r.t.Fatalf("sealed ref %q, want the crf1. form", ref)
	}
	return ref
}

func TestEachCredentialRefRouteAdmitsOnlyItsOwnPrincipalKey(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	app := rig.app(cbAdminPrincipal(cbOrg))
	sealBody := cbSealBody(id, cbOrg, cbHost, time.Now().Add(time.Minute), boolRef(false), boolRef(false))
	for _, c := range []struct {
		name, path string
		headers    map[string]string
	}{
		{"seal with the shared internal key", credentialRefSealPath, keyHeaders("dev-key")},
		{"seal with execution-core's key", credentialRefSealPath, keyHeaders(cbRedeemKey)},
		{"seal with a bearer", credentialRefSealPath, bearerHeaders()},
		{"seal with no key", credentialRefSealPath, nil},
		{"redeem with the shared internal key", credentialRefRedeemPath, keyHeaders("dev-key")},
		{"redeem with model-gateway's key", credentialRefRedeemPath, keyHeaders(cbSealKey)},
		{"redeem with a bearer", credentialRefRedeemPath, bearerHeaders()},
	} {
		resp := cbSend(t, app, http.MethodPost, c.path, sealBody, c.headers)
		if resp.status != http.StatusUnauthorized {
			t.Errorf("%s answered %d %s, want 401", c.name, resp.status, resp.raw)
		}
		if strings.Contains(resp.raw, "crf1.") || strings.Contains(resp.raw, cbSecret) {
			t.Errorf("%s leaked a reference or the secret", c.name)
		}
	}

	// A principal with no configured key cannot be reached with an empty or any other key.
	cfg, _, service := testOAuthStack(t)
	cfg.CredentialBindingsEnabled = true
	cfg.CredentialRefSealKeys = "model-gateway=" + cbSealKey
	only := NewServer(ServerConfig{Config: cfg, Repo: rig.mem, OAuth: service, OrgZDR: rig.posture, CredentialSealer: rig.vault, CredentialHostVetter: rig.vetter, Auth: fakeVerifier{}})
	if resp := cbSend(t, only, http.MethodPost, credentialRefRedeemPath, `{}`, keyHeaders(cbSealKey)); resp.status != http.StatusUnauthorized {
		t.Fatalf("redeem without an execution-core key configured answered %d, want 401", resp.status)
	}
}

func TestSealRefusesZDROrWaivedEvidenceBeforeTheBindingIsRead(t *testing.T) {
	rig := newCBRig(t)
	// The binding does not exist: a request that reached the store would answer 404, not the refusal.
	missing := credentialBindingIDPrefix + "missing"
	for _, c := range []struct {
		name        string
		zdr, waived *bool
		status      int
		code        string
	}{
		{"zdr", boolRef(true), boolRef(false), http.StatusForbidden, "credential_ref_refused"},
		{"waived", boolRef(false), boolRef(true), http.StatusForbidden, "credential_ref_refused"},
		{"both", boolRef(true), boolRef(true), http.StatusForbidden, "credential_ref_refused"},
		{"zdr absent", nil, boolRef(false), http.StatusBadRequest, "invalid_evidence"},
		{"waived absent", boolRef(false), nil, http.StatusBadRequest, "invalid_evidence"},
	} {
		resp := rig.seal(cbSealBody(missing, cbOrg, cbHost, time.Now().Add(time.Minute), c.zdr, c.waived))
		if resp.status != c.status || errorCode(resp.body) != c.code {
			t.Errorf("%s: seal answered %d %s, want %d %s", c.name, resp.status, resp.raw, c.status, c.code)
		}
		if c.status == http.StatusForbidden && reasonCode(resp.body) != credentialRefReasonZDROrWaived {
			t.Errorf("%s: refusal reason %q, want %s", c.name, reasonCode(resp.body), credentialRefReasonZDROrWaived)
		}
		if strings.Contains(resp.raw, "crf1.") {
			t.Errorf("%s: a reference was issued", c.name)
		}
	}
	if len(drainAudit(t, rig.mem)) != 0 {
		t.Fatal("a refused seal wrote an audit row; sealing writes nothing")
	}
}

func TestSealRefusesAnInactiveBindingAnotherHostAnotherOrganisationAndAPassedDeadline(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	revoked := rig.createBinding(cbOrg, "other.example.com")
	if resp := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodDelete, credentialBindingsPath+"/"+revoked, "", bearerHeaders()); resp.status != http.StatusOK {
		t.Fatalf("revoke: %d", resp.status)
	}
	soon := time.Now().Add(time.Minute)
	for _, c := range []struct {
		name   string
		body   string
		status int
		code   string
		reason string
	}{
		{"a revoked binding", cbSealBody(revoked, cbOrg, "other.example.com", soon, boolRef(false), boolRef(false)), http.StatusConflict, "credential_binding_inactive", ""},
		{"a host the binding does not name", cbSealBody(id, cbOrg, "evil.example.net", soon, boolRef(false), boolRef(false)), http.StatusForbidden, "credential_ref_refused", credentialRefReasonHostMismatch},
		{"another organisation's binding", cbSealBody(id, cbOtherOrg, cbHost, soon, boolRef(false), boolRef(false)), http.StatusNotFound, "credential_binding_not_found", ""},
		{"a deadline long past", cbSealBody(id, cbOrg, cbHost, time.Now().Add(-time.Hour), boolRef(false), boolRef(false)), http.StatusConflict, "call_deadline_passed", ""},
		{"an IP host", cbSealBody(id, cbOrg, "93.184.216.34", soon, boolRef(false), boolRef(false)), http.StatusBadRequest, "invalid_host", ""},
		{"a slot with a dot", strings.Replace(cbSealBody(id, cbOrg, cbHost, soon, boolRef(false), boolRef(false)), `"api_key"`, `"../x"`, 1), http.StatusBadRequest, "invalid_slot", ""},
	} {
		resp := rig.seal(c.body)
		if resp.status != c.status || errorCode(resp.body) != c.code || (c.reason != "" && reasonCode(resp.body) != c.reason) {
			t.Errorf("%s: seal answered %d %s, want %d %s %s", c.name, resp.status, resp.raw, c.status, c.code, c.reason)
		}
		if strings.Contains(resp.raw, "crf1.") {
			t.Errorf("%s: a reference was issued", c.name)
		}
	}
}

func TestAReferenceLastsUntilTheDeadlinePlusThirtySecondsAndNoMoreThanFifteenMinutes(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	refs, err := secretcrypto.NewCredentialRefs(rig.key)
	if err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct {
		name     string
		deadline time.Duration
		want     func(now, deadline time.Time) time.Time
	}{
		{"a near deadline", 2 * time.Minute, func(_, deadline time.Time) time.Time { return deadline.Add(30 * time.Second) }},
		{"a far deadline", 3 * time.Hour, func(now, _ time.Time) time.Time { return now.Add(15 * time.Minute) }},
	} {
		now := time.Now().UTC()
		deadline := now.Add(c.deadline)
		resp := rig.seal(cbSealBody(id, cbOrg, cbHost, deadline, boolRef(false), boolRef(false)))
		if resp.status != http.StatusOK {
			t.Fatalf("%s: seal %d %s", c.name, resp.status, resp.raw)
		}
		if resp.header.Get("Cache-Control") != "no-store" {
			t.Errorf("%s: seal answer is cacheable", c.name)
		}
		ref, _ := dataOf(resp.body)["ref"].(string)
		claims, err := refs.Open(ref, cbHost, "call-1")
		if err != nil {
			t.Fatalf("%s: open the sealed ref: %v", c.name, err)
		}
		want := c.want(now, deadline)
		if diff := claims.ExpiresAt.Sub(want); diff > 2*time.Second || diff < -2*time.Second {
			t.Errorf("%s: reference expires at %s, want about %s", c.name, claims.ExpiresAt, want)
		}
		if claims.BindingID != id || claims.OrgID != cbOrg || claims.ConnectionID != "conn-1" || claims.Slot != "api_key" {
			t.Errorf("%s: the reference names %+v, not the requested scope", c.name, claims)
		}
		if strings.Contains(ref, cbSecret) {
			t.Errorf("%s: the reference holds the secret", c.name)
		}
	}
}

func TestRedeemReleasesTheSecretOnlyAfterItsAuditRowAndTheRowCarriesNoSecretOrReference(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	ref := rig.sealedRef(id)
	drainAudit(t, rig.mem)

	resp := rig.redeem(ref, cbHost, "call-1")
	if resp.status != http.StatusOK || dataOf(resp.body)["secret"] != cbSecret || dataOf(resp.body)["header_name"] != "Authorization" {
		t.Fatalf("redeem answered %d %s, want 200 with the secret and its header", resp.status, resp.raw)
	}
	if resp.header.Get("Cache-Control") != "no-store" {
		t.Fatal("the redemption answer is cacheable")
	}
	rows := auditOfType(drainAudit(t, rig.mem), auditEventCredentialRedeemed)
	if len(rows) != 1 {
		t.Fatalf("redeem wrote %d redemption rows, want 1", len(rows))
	}
	row := rows[0]
	if row.OrganizationID != cbOrg || row.ConnectionID != "conn-1" || row.Metadata["outcome"] != credentialRefOutcomeAllowed ||
		row.Metadata["reasonCode"] != credentialRefReasonOK || row.Metadata["bindingId"] != id || row.Metadata["callId"] != "call-1" {
		t.Fatalf("redemption row %+v, want the organisation, connection, binding, call and outcome allowed/ok", row)
	}
	dump := fmt.Sprintf("%+v", row)
	if strings.Contains(dump, cbSecret) || strings.Contains(dump, ref) || strings.Contains(dump, strings.TrimPrefix(ref, "crf1.")) {
		t.Fatalf("the redemption row carries the secret or the reference: %s", dump)
	}

	// A second redemption is a second release and a second row: nothing is cached.
	if again := rig.redeem(ref, cbHost, "call-1"); again.status != http.StatusOK {
		t.Fatalf("second redeem: %d", again.status)
	}
	if rows := auditOfType(drainAudit(t, rig.mem), auditEventCredentialRedeemed); len(rows) != 1 {
		t.Fatalf("the second redemption wrote %d rows, want its own one", len(rows))
	}
}

func TestRedeemRefusesAnotherHostAnotherCallAnExpiredReferenceAndARevokedBindingAndAuditsEach(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	ref := rig.sealedRef(id)
	refs, err := secretcrypto.NewCredentialRefs(rig.key)
	if err != nil {
		t.Fatal(err)
	}
	expired, err := refs.Seal(id, cbOrg, "conn-1", "call-1", "api_key", cbHost, time.Now().Add(-time.Second))
	if err != nil {
		t.Fatal(err)
	}
	drainAudit(t, rig.mem)

	check := func(name string, resp cbResponse, reason string) {
		t.Helper()
		if resp.status != http.StatusForbidden || errorCode(resp.body) != "credential_ref_refused" || reasonCode(resp.body) != reason {
			t.Errorf("%s: redeem answered %d %s, want 403 credential_ref_refused %s", name, resp.status, resp.raw, reason)
		}
		if strings.Contains(resp.raw, cbSecret) {
			t.Errorf("%s: the refusal carries the secret", name)
		}
		rows := auditOfType(drainAudit(t, rig.mem), auditEventCredentialRedeemed)
		if len(rows) != 1 || rows[0].Metadata["outcome"] != credentialRefOutcomeRefused || rows[0].Metadata["reasonCode"] != reason || rows[0].Metadata["bindingId"] != id {
			t.Errorf("%s: redemption rows %+v, want one refused row with reason %s naming the binding", name, rows, reason)
		}
	}
	check("another host", rig.redeem(ref, "other.example.com", "call-1"), credentialRefReasonHostMismatch)
	check("another call", rig.redeem(ref, cbHost, "call-2"), credentialRefReasonCallMismatch)
	check("an expired reference", rig.redeem(expired, cbHost, "call-1"), credentialRefReasonExpired)

	if resp := cbSend(t, rig.app(cbAdminPrincipal(cbOrg)), http.MethodDelete, credentialBindingsPath+"/"+id, "", bearerHeaders()); resp.status != http.StatusOK {
		t.Fatalf("revoke: %d", resp.status)
	}
	drainAudit(t, rig.mem)
	check("a binding revoked after sealing", rig.redeem(ref, cbHost, "call-1"), credentialRefReasonBindingRevoked)

	// A reference that does not authenticate names nothing: it is refused and no row can name an organisation.
	tampered := ref[:len(ref)-2] + "AA"
	if tampered == ref {
		tampered = ref[:len(ref)-2] + "BB"
	}
	for name, bad := range map[string]string{"a tampered reference": tampered, "a reference under another key": func() string {
		other, _ := secretcrypto.NewCredentialRefs([]byte("abcdefghijklmnopqrstuvwxyz012345"))
		sealed, _ := other.Seal(id, cbOrg, "conn-1", "call-1", "api_key", cbHost, time.Now().Add(time.Minute))
		return sealed
	}(), "not a reference": "token-123"} {
		resp := rig.redeem(bad, cbHost, "call-1")
		if resp.status != http.StatusForbidden || errorCode(resp.body) != "credential_ref_invalid" {
			t.Errorf("%s: redeem answered %d %s, want 403 credential_ref_invalid", name, resp.status, resp.raw)
		}
		if rows := drainAudit(t, rig.mem); len(rows) != 0 {
			t.Errorf("%s wrote audit rows %+v", name, rows)
		}
	}
}

func TestRedeemFailsClosedWhenItsAuditRowCannotBeWritten(t *testing.T) {
	rig := newCBRig(t)
	id := rig.createBinding(cbOrg, cbHost)
	ref := rig.sealedRef(id)
	rig.repo = auditInsertFailureRepository{Repository: rig.mem}

	resp := rig.redeem(ref, cbHost, "call-1")
	if resp.status != http.StatusServiceUnavailable || errorCode(resp.body) != "credential_audit_failed" {
		t.Fatalf("redeem with an unwritable audit row answered %d %s, want 503 credential_audit_failed", resp.status, resp.raw)
	}
	if strings.Contains(resp.raw, cbSecret) {
		t.Fatal("the secret left although its redemption row was not written")
	}
	// A refusal whose row cannot be written is not silent either.
	refused := rig.redeem(ref, "other.example.com", "call-1")
	if refused.status != http.StatusServiceUnavailable || errorCode(refused.body) != "credential_audit_failed" {
		t.Fatalf("a refused redemption with an unwritable row answered %d %s, want 503 credential_audit_failed", refused.status, refused.raw)
	}
}
