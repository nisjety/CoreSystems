package http

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/I-Dacosta/AquatiqCMS/apps/org-core/internal/orgkeys"
	"github.com/gin-gonic/gin"
)

const (
	keysOrgA         = "org-keys-a"
	keysOrgB         = "org-keys-b"
	keysQuarryToken  = "1111111111111111111111111111111111111111"
	keysBillingToken = "2222222222222222222222222222222222222222"
	keysAuthToken    = "3333333333333333333333333333333333333333"
	keysGatewayToken = "4444444444444444444444444444444444444444"
	keysProfileBody  = `{"purpose":"quarry.profile","context":{"profile_id":"prof_a"}}`
)

var keysMasterB64 = base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{0x5a}, orgkeys.KeySize))

func keysRegistry() string {
	return fmt.Sprintf(`[
  {"principal":"verevon-gateway","audience":"org-core","token":%q,"scopes":["org:read:self","org:provision:self","org:settings:write:self","org:onboarding:write:self"]},
  {"principal":"auth-core","audience":"org-core","token":%q,"scopes":["org:projection:write:any","org:projection:delete:any","org:retention:read:any"]},
  {"principal":"billing-core","audience":"org-core","token":%q,"scopes":["org:read:any"]},
  {"principal":"quarry-edge","audience":"org-core","token":%q,"scopes":["org:retention:read:any","org:keys:wrap:any","org:keys:unwrap:any"]}
]`, keysGatewayToken, keysAuthToken, keysBillingToken, keysQuarryToken)
}

var keysTokens = map[string]string{
	"verevon-gateway": keysGatewayToken,
	"auth-core":       keysAuthToken,
	"billing-core":    keysBillingToken,
	"quarry-edge":     keysQuarryToken,
}

type keysHarness struct {
	t       *testing.T
	server  *Server
	store   *orgkeys.MemoryStore
	service *orgkeys.Service
	// admin serves the admin and destroy handlers behind a stand-in principal:
	// their scopes are not allowlisted yet, so no registry credential reaches them.
	admin *gin.Engine
}

func keysEnv(values map[string]string) func(string) string {
	return func(name string) string { return values[name] }
}

func newKeysHarness(t *testing.T) *keysHarness {
	t.Helper()
	t.Setenv(serviceCredentialEnv, keysRegistry())
	t.Setenv(orgkeys.EnvEnabled, "1")
	server := NewServer(0, nil, nil, "", "")
	store := orgkeys.NewMemoryStore(keysOrgA, keysOrgB)
	master, err := orgkeys.LoadEnvMasterKeys(keysEnv(map[string]string{
		orgkeys.EnvMasterKeys:   `{"v1":"` + keysMasterB64 + `"}`,
		orgkeys.EnvMasterActive: "v1",
	}))
	if err != nil {
		t.Fatalf("master keys: %v", err)
	}
	service := orgkeys.NewService(store, master)
	server.SetOrgKeys(service)

	admin := gin.New()
	admin.Use(func(c *gin.Context) { c.Set("service_id", "operator"); c.Next() })
	keys := admin.Group("/internal/orgs/:orgId/keys")
	keys.POST("/rotate", server.rotateOrgKeys)
	keys.GET("", server.getOrgKeysMetadata)
	keys.DELETE("", server.destroyOrgKeys)
	return &keysHarness{t: t, server: server, store: store, service: service, admin: admin}
}

func keysPath(orgID, suffix string) string {
	return "/internal/orgs/" + orgID + "/keys" + suffix
}

func (h *keysHarness) do(principal, method, path, body string) *httptest.ResponseRecorder {
	h.t.Helper()
	var reader *strings.Reader
	if body != "" {
		reader = strings.NewReader(body)
	} else {
		reader = strings.NewReader("")
	}
	request := httptest.NewRequest(method, path, reader)
	request.Header.Set("Content-Type", "application/json")
	request.Header.Set("X-Service-Id", principal)
	request.Header.Set("X-Service-Token", keysTokens[principal])
	recorder := httptest.NewRecorder()
	h.server.router.ServeHTTP(recorder, request)
	return recorder
}

func (h *keysHarness) doAdmin(method, path string) *httptest.ResponseRecorder {
	h.t.Helper()
	recorder := httptest.NewRecorder()
	h.admin.ServeHTTP(recorder, httptest.NewRequest(method, path, nil))
	return recorder
}

func keysJSON(t *testing.T, recorder *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	var body map[string]any
	if err := json.Unmarshal(recorder.Body.Bytes(), &body); err != nil {
		t.Fatalf("body is not JSON: %s", recorder.Body.String())
	}
	return body
}

func keysData(t *testing.T, recorder *httptest.ResponseRecorder) map[string]any {
	t.Helper()
	data, ok := keysJSON(t, recorder)["data"].(map[string]any)
	if !ok {
		t.Fatalf("not a {data} envelope: %s", recorder.Body.String())
	}
	return data
}

func keysErrorCode(t *testing.T, recorder *httptest.ResponseRecorder) string {
	t.Helper()
	envelope, ok := keysJSON(t, recorder)["error"].(map[string]any)
	if !ok {
		return ""
	}
	code, _ := envelope["code"].(string)
	return code
}

func wrappedBody(keyID, wrapped, profileID string) string {
	return fmt.Sprintf(`{"purpose":"quarry.profile","context":{"profile_id":%q},"key_id":%q,"wrapped_dek_b64":%q}`, profileID, keyID, wrapped)
}

func (h *keysHarness) generate(orgID string) (keyID, dek, wrapped string) {
	h.t.Helper()
	response := h.do("quarry-edge", http.MethodPost, keysPath(orgID, "/data-keys"), keysProfileBody)
	if response.Code != http.StatusCreated {
		h.t.Fatalf("data-keys: status=%d body=%s", response.Code, response.Body.String())
	}
	data := keysData(h.t, response)
	return data["key_id"].(string), data["dek_b64"].(string), data["wrapped_dek_b64"].(string)
}

func TestOrgKeysRoutesAbsentWhenDisabled(t *testing.T) {
	t.Setenv(serviceCredentialEnv, keysRegistry())
	t.Setenv(orgkeys.EnvEnabled, "0")
	server := NewServer(0, nil, nil, "", "")
	for _, route := range server.router.Routes() {
		if strings.Contains(route.Path, "/keys") {
			t.Fatalf("key route %s %s mounted with the flag off", route.Method, route.Path)
		}
	}
	request := httptest.NewRequest(http.MethodPost, keysPath(keysOrgA, "/data-keys"), strings.NewReader(keysProfileBody))
	request.Header.Set("X-Service-Id", "quarry-edge")
	request.Header.Set("X-Service-Token", keysQuarryToken)
	recorder := httptest.NewRecorder()
	server.router.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusNotFound {
		t.Fatalf("flag off: status=%d, want 404", recorder.Code)
	}
}

func TestOrgKeysRoutesMountedWhenEnabled(t *testing.T) {
	h := newKeysHarness(t)
	want := map[string]bool{
		"POST /internal/orgs/:orgId/keys/data-keys":        false,
		"POST /internal/orgs/:orgId/keys/data-keys/unwrap": false,
		"POST /internal/orgs/:orgId/keys/data-keys/rewrap": false,
		"POST /internal/orgs/:orgId/keys/rotate":           false,
		"GET /internal/orgs/:orgId/keys":                   false,
		"DELETE /internal/orgs/:orgId/keys":                false,
	}
	for _, route := range h.server.router.Routes() {
		key := route.Method + " " + route.Path
		if _, ok := want[key]; ok {
			want[key] = true
		} else if strings.Contains(route.Path, "/keys") {
			t.Fatalf("unexpected key route %s", key)
		}
	}
	for key, mounted := range want {
		if !mounted {
			t.Errorf("%s is not mounted", key)
		}
	}
}

func TestOrgKeysGenerateUnwrapRoundtrip(t *testing.T) {
	h := newKeysHarness(t)
	response := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys"), keysProfileBody)
	if response.Code != http.StatusCreated {
		t.Fatalf("data-keys: status=%d body=%s", response.Code, response.Body.String())
	}
	if response.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("Cache-Control = %q, want no-store", response.Header().Get("Cache-Control"))
	}
	data := keysData(t, response)
	if data["key_id"] != "oc:1" || data["kek_version"] != float64(1) || data["alg"] != "A256GCM" {
		t.Fatalf("data-keys answer: %v", data)
	}
	dek, wrapped := data["dek_b64"].(string), data["wrapped_dek_b64"].(string)
	if raw, err := base64.StdEncoding.DecodeString(dek); err != nil || len(raw) != orgkeys.KeySize {
		t.Fatalf("dek_b64 is not a 32-byte key: %v", err)
	}
	unwrap := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/unwrap"), wrappedBody("oc:1", wrapped, "prof_a"))
	if unwrap.Code != http.StatusOK || unwrap.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("unwrap: status=%d body=%s", unwrap.Code, unwrap.Body.String())
	}
	got := keysData(t, unwrap)
	if got["dek_b64"] != dek || got["current_kek_version"] != float64(1) {
		t.Fatalf("unwrap answer does not match the generated key")
	}
}

func TestOrgKeysUnwrapWithWrongContextFails(t *testing.T) {
	h := newKeysHarness(t)
	keyID, _, wrapped := h.generate(keysOrgA)
	wrongProfile := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/unwrap"), wrappedBody(keyID, wrapped, "prof_b"))
	if wrongProfile.Code != http.StatusUnprocessableEntity || keysErrorCode(t, wrongProfile) != "invalid_context" {
		t.Fatalf("wrong profile: status=%d body=%s", wrongProfile.Code, wrongProfile.Body.String())
	}
	// Org A's blob through org B's path: org B has no v1 yet, then has its own.
	foreign := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgB, "/data-keys/unwrap"), wrappedBody(keyID, wrapped, "prof_a"))
	if foreign.Code != http.StatusNotFound || keysErrorCode(t, foreign) != "key_not_found" {
		t.Fatalf("foreign org without kek: status=%d body=%s", foreign.Code, foreign.Body.String())
	}
	h.generate(keysOrgB)
	foreign = h.do("quarry-edge", http.MethodPost, keysPath(keysOrgB, "/data-keys/unwrap"), wrappedBody(keyID, wrapped, "prof_a"))
	if foreign.Code != http.StatusUnprocessableEntity || keysErrorCode(t, foreign) != "invalid_context" {
		t.Fatalf("foreign org: status=%d body=%s", foreign.Code, foreign.Body.String())
	}
	for name, body := range map[string]string{
		"unknown field":   `{"purpose":"quarry.profile","context":{"profile_id":"prof_a"},"org_id":"org-keys-b"}`,
		"wrapped on mint": wrappedBody(keyID, wrapped, "prof_a"),
		"no context":      `{"purpose":"quarry.profile"}`,
		"two objects":     keysProfileBody + keysProfileBody,
	} {
		if response := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys"), body); response.Code != http.StatusBadRequest {
			t.Fatalf("%s: status=%d, want 400", name, response.Code)
		}
	}
	if response := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys"), `{"purpose":"other","context":{"profile_id":"prof_a"}}`); response.Code != http.StatusUnprocessableEntity {
		t.Fatalf("unknown purpose: status=%d, want 422", response.Code)
	}
	if response := h.do("quarry-edge", http.MethodPost, keysPath("org-missing", "/data-keys"), keysProfileBody); response.Code != http.StatusNotFound || keysErrorCode(t, response) != "org_not_found" {
		t.Fatalf("unknown org: status=%d body=%s", response.Code, response.Body.String())
	}
}

func TestOrgKeysScopesGateEachRoute(t *testing.T) {
	h := newKeysHarness(t)
	_, _, wrapped := h.generate(keysOrgA)
	routes := []struct {
		method, suffix, body string
		quarry               bool // quarry-edge holds the scope
	}{
		{http.MethodPost, "/data-keys", keysProfileBody, true},
		{http.MethodPost, "/data-keys/unwrap", wrappedBody("oc:1", wrapped, "prof_a"), true},
		{http.MethodPost, "/data-keys/rewrap", wrappedBody("oc:1", wrapped, "prof_a"), true},
		{http.MethodPost, "/rotate", "", false},
		{http.MethodGet, "", "", false},
		{http.MethodDelete, "", "", false},
	}
	for _, route := range routes {
		path := keysPath(keysOrgA, route.suffix)
		for _, principal := range []string{"verevon-gateway", "auth-core", "billing-core"} {
			if response := h.do(principal, route.method, path, route.body); response.Code != http.StatusForbidden {
				t.Fatalf("%s %s as %s: status=%d, want 403", route.method, path, principal, response.Code)
			}
		}
		response := h.do("quarry-edge", route.method, path, route.body)
		if route.quarry && response.Code == http.StatusForbidden {
			t.Fatalf("%s %s as quarry-edge: 403, want served", route.method, path)
		}
		if !route.quarry && response.Code != http.StatusForbidden {
			t.Fatalf("%s %s as quarry-edge: status=%d, want 403 (admin/destroy scope not held)", route.method, path, response.Code)
		}
	}
	// The required scope of each path, as the middleware reads it.
	for path, want := range map[string]string{
		"POST " + keysPath(keysOrgA, "/data-keys"):        "org:keys:wrap:any",
		"POST " + keysPath(keysOrgA, "/data-keys/unwrap"): "org:keys:unwrap:any",
		"POST " + keysPath(keysOrgA, "/data-keys/rewrap"): "org:keys:wrap:any",
		"POST " + keysPath(keysOrgA, "/rotate"):           "org:keys:admin:any",
		"GET " + keysPath(keysOrgA, ""):                   "org:keys:admin:any",
		"DELETE " + keysPath(keysOrgA, ""):                "org:keys:destroy:any",
	} {
		parts := strings.SplitN(path, " ", 2)
		scopes := serviceScopesForRequest(httptest.NewRequest(parts[0], parts[1], nil))
		if len(scopes) != 1 || scopes[0] != want {
			t.Fatalf("%s: scopes=%v want [%s]", path, scopes, want)
		}
	}
	// The wrap and unwrap scopes are pinned to quarry-edge.
	for _, scope := range []string{"org:keys:wrap:any", "org:keys:unwrap:any"} {
		if _, err := parseServiceCredentials(`[{"principal":"billing-core","audience":"org-core","token":"abcdefabcdefabcdefabcdefabcdefabcdef","scopes":["` + scope + `"]}]`); err == nil {
			t.Fatalf("%s accepted for billing-core", scope)
		}
	}
}

func TestOrgKeysDestroyMakesUnwrap410(t *testing.T) {
	h := newKeysHarness(t)
	keyID, _, wrapped := h.generate(keysOrgA)
	destroy := h.doAdmin(http.MethodDelete, keysPath(keysOrgA, ""))
	if destroy.Code != http.StatusOK || keysData(t, destroy)["versions_destroyed"] != float64(1) {
		t.Fatalf("destroy: status=%d body=%s", destroy.Code, destroy.Body.String())
	}
	unwrap := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/unwrap"), wrappedBody(keyID, wrapped, "prof_a"))
	if unwrap.Code != http.StatusGone || keysErrorCode(t, unwrap) != "key_destroyed" {
		t.Fatalf("unwrap after destroy: status=%d body=%s", unwrap.Code, unwrap.Body.String())
	}
	mint := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys"), keysProfileBody)
	if mint.Code != http.StatusGone {
		t.Fatalf("data-keys after destroy: status=%d, want 410", mint.Code)
	}
	if again := h.doAdmin(http.MethodDelete, keysPath(keysOrgA, "")); again.Code != http.StatusGone {
		t.Fatalf("second destroy: status=%d, want 410", again.Code)
	}
	meta := h.doAdmin(http.MethodGet, keysPath(keysOrgA, ""))
	if meta.Code != http.StatusOK || keysData(t, meta)["state"] != "destroyed" {
		t.Fatalf("metadata after destroy: status=%d body=%s", meta.Code, meta.Body.String())
	}
}

func TestOrgKeysRotateThenRewrapKeepsDEK(t *testing.T) {
	h := newKeysHarness(t)
	keyID, dek, wrapped := h.generate(keysOrgA)
	rotate := h.doAdmin(http.MethodPost, keysPath(keysOrgA, "/rotate"))
	if rotate.Code != http.StatusOK || keysData(t, rotate)["kek_version"] != float64(2) {
		t.Fatalf("rotate: status=%d body=%s", rotate.Code, rotate.Body.String())
	}
	rewrap := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/rewrap"), wrappedBody(keyID, wrapped, "prof_a"))
	if rewrap.Code != http.StatusOK {
		t.Fatalf("rewrap: status=%d body=%s", rewrap.Code, rewrap.Body.String())
	}
	data := keysData(t, rewrap)
	if data["key_id"] != "oc:2" || data["dek_b64"] != nil {
		t.Fatalf("rewrap answer: %v (the DEK must not leave org-core)", data)
	}
	unwrap := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/unwrap"), wrappedBody("oc:2", data["wrapped_dek_b64"].(string), "prof_a"))
	if unwrap.Code != http.StatusOK || keysData(t, unwrap)["dek_b64"] != dek {
		t.Fatalf("unwrap rewrapped: status=%d", unwrap.Code)
	}
	meta := keysData(t, h.doAdmin(http.MethodGet, keysPath(keysOrgA, "")))
	if meta["active_kek_version"] != float64(2) || len(meta["versions"].([]any)) != 2 {
		t.Fatalf("metadata: %v", meta)
	}
	rendered, _ := json.Marshal(meta)
	if strings.Contains(string(rendered), "wrapped") || strings.Contains(string(rendered), keysMasterB64) {
		t.Fatalf("metadata carries key material: %s", rendered)
	}
}

func TestOrgKeysCanaryFailureReturns503(t *testing.T) {
	h := newKeysHarness(t)
	// A canary sealed under another key for v1: the configured master key is wrong.
	other := orgkeys.NewMemoryStore()
	otherMaster, err := orgkeys.LoadEnvMasterKeys(keysEnv(map[string]string{
		orgkeys.EnvMasterKeys:   `{"v1":"` + base64.StdEncoding.EncodeToString(bytes.Repeat([]byte{0x6b}, orgkeys.KeySize)) + `"}`,
		orgkeys.EnvMasterActive: "v1",
	}))
	if err != nil {
		t.Fatalf("other master: %v", err)
	}
	if err := orgkeys.NewService(other, otherMaster).Ready(context.Background()); err != nil {
		t.Fatalf("other ready: %v", err)
	}
	sealed, _ := other.Canaries(context.Background())
	h.store.SetCanary("v1", sealed["v1"])

	response := h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys"), keysProfileBody)
	if response.Code != http.StatusServiceUnavailable || keysErrorCode(t, response) != "keys_unavailable" {
		t.Fatalf("canary failure: status=%d body=%s", response.Code, response.Body.String())
	}
	if strings.Contains(strings.ToLower(response.Body.String()), "decrypt") {
		t.Fatalf("canary failure reads as a decrypt error: %s", response.Body.String())
	}
}

func TestOrgKeysWithoutServiceAnswer503(t *testing.T) {
	t.Setenv(serviceCredentialEnv, keysRegistry())
	t.Setenv(orgkeys.EnvEnabled, "1")
	server := NewServer(0, nil, nil, "", "")
	server.SetOrgKeys(nil)
	request := httptest.NewRequest(http.MethodPost, keysPath(keysOrgA, "/data-keys"), strings.NewReader(keysProfileBody))
	request.Header.Set("X-Service-Id", "quarry-edge")
	request.Header.Set("X-Service-Token", keysQuarryToken)
	recorder := httptest.NewRecorder()
	server.router.ServeHTTP(recorder, request)
	if recorder.Code != http.StatusServiceUnavailable {
		t.Fatalf("no service: status=%d, want 503", recorder.Code)
	}
}

func TestOrgKeysNoDEKOrWrappedDEKInLogs(t *testing.T) {
	var logs bytes.Buffer
	previousLog := log.Writer()
	log.SetOutput(&logs)
	previousGin, previousGinErr := gin.DefaultWriter, gin.DefaultErrorWriter
	gin.DefaultWriter, gin.DefaultErrorWriter = &logs, &logs
	t.Cleanup(func() {
		log.SetOutput(previousLog)
		gin.DefaultWriter, gin.DefaultErrorWriter = previousGin, previousGinErr
	})

	h := newKeysHarness(t)
	keyID, dek, wrapped := h.generate(keysOrgA)
	h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/unwrap"), wrappedBody(keyID, wrapped, "prof_a"))
	h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/unwrap"), wrappedBody(keyID, wrapped, "prof_b"))
	h.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys/rewrap"), wrappedBody(keyID, wrapped, "prof_a"))
	h.doAdmin(http.MethodPost, keysPath(keysOrgA, "/rotate"))
	h.doAdmin(http.MethodGet, keysPath(keysOrgA, ""))
	h.doAdmin(http.MethodDelete, keysPath(keysOrgA, ""))
	// And the 503 path, which is the one that logs an error text.
	broken := newKeysHarness(t)
	broken.store.SetCanary("v1", []byte("not-a-canary-but-long-enough-to-try-open"))
	broken.do("quarry-edge", http.MethodPost, keysPath(keysOrgA, "/data-keys"), keysProfileBody)

	rawDEK, _ := base64.StdEncoding.DecodeString(dek)
	rawWrapped, _ := base64.StdEncoding.DecodeString(wrapped)
	for name, secret := range map[string]string{
		"dek_b64":         dek,
		"wrapped_dek_b64": wrapped,
		"master key":      keysMasterB64,
		"raw dek":         string(rawDEK),
		"raw wrapped":     string(rawWrapped),
		"service token":   keysQuarryToken,
	} {
		if strings.Contains(logs.String(), secret) {
			t.Fatalf("logs contain the %s", name)
		}
	}
	for _, payload := range h.store.AuditPayloads() {
		rendered, _ := json.Marshal(payload)
		if strings.Contains(string(rendered), dek) || strings.Contains(string(rendered), wrapped) {
			t.Fatalf("audit payload carries key material: %s", rendered)
		}
	}
}

func TestValidateOrgKeysConfig(t *testing.T) {
	if err := ValidateOrgKeysConfig(keysEnv(map[string]string{})); err != nil {
		t.Fatalf("flag off must pass: %v", err)
	}
	if err := ValidateOrgKeysConfig(keysEnv(map[string]string{orgkeys.EnvEnabled: "1"})); err == nil {
		t.Fatal("flag on without master keys must refuse to boot")
	} else if strings.Contains(err.Error(), keysMasterB64) {
		t.Fatal("boot error echoes a key")
	}
}
